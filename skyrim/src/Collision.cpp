// Skyrim's collision around the player, as triangles for Faith's movement (faith_set_world).
//
// Walks the cell's Havok world and copies out every triangle, box, capsule and convex hull that
// touches a box around the player, in game units. The shape walking follows SkyCraft's
// Collision.cpp (MIT License, Copyright (c) 2026 chasmlol, github.com/chasmlol/SkyCraft),
// which reads the same shapes on the same runtime (1.7.104).
#include "Collision.h"

namespace faith::Collision
{
	namespace
	{
		struct Tri
		{
			float v[9];
		};
		struct Obb
		{
			float c[3];
			float axis[3][3];
			float half[3];
		};
		struct Capsule
		{
			float a[3], b[3];
			float r;
		};
		struct Convex
		{
			std::vector<std::array<float, 4>> planes;  // n.p + d <= 0 inside
			float                             lo[3], hi[3];
		};
		struct Job
		{
			std::vector<Tri>     tris;
			std::vector<Obb>     boxes;
			std::vector<Capsule> capsules;
			std::vector<Convex>  convexes;
		};

		constexpr int kMaxKeys = 16384;

		bool Finite(const float* a_v, int a_n)
		{
			for (int i = 0; i < a_n; ++i) {
				if (!std::isfinite(a_v[i]) || std::fabs(a_v[i]) > 1.0e7f) {
					return false;
				}
			}
			return true;
		}

		// Havok transform: rotation columns at [0..2], [4..6], [8..10]; translation at [12..14].
		void XfPoint(const float* a_xf, const float* a_p, float* a_out)
		{
			for (int i = 0; i < 3; ++i) {
				a_out[i] = a_xf[i] * a_p[0] + a_xf[4 + i] * a_p[1] + a_xf[8 + i] * a_p[2] + a_xf[12 + i];
			}
		}

		void XfDir(const float* a_xf, const float* a_d, float* a_out)
		{
			for (int i = 0; i < 3; ++i) {
				a_out[i] = a_xf[i] * a_d[0] + a_xf[4 + i] * a_d[1] + a_xf[8 + i] * a_d[2];
			}
		}

		void XfCompose(const float* a_parent, const float* a_child, float* a_out)
		{
			for (int c = 0; c < 3; ++c) {
				XfDir(a_parent, a_child + c * 4, a_out + c * 4);
				a_out[c * 4 + 3] = 0.0f;
			}
			XfPoint(a_parent, a_child + 12, a_out + 12);
			a_out[15] = 1.0f;
		}

		bool XfLooksValid(const float* a_xf)
		{
			if (!Finite(a_xf, 16)) {
				return false;
			}
			for (int c = 0; c < 3; ++c) {
				const float* col = a_xf + c * 4;
				const float  len = col[0] * col[0] + col[1] * col[1] + col[2] * col[2];
				if (std::fabs(len - 1.0f) > 0.05f) {
					return false;
				}
			}
			return true;
		}

		void AabbToGame(const RE::hkAabb& a_box, float a_k, float* a_lo, float* a_hi)
		{
			alignas(16) float mn[4], mx[4];
			_mm_store_ps(mn, a_box.min.quad);
			_mm_store_ps(mx, a_box.max.quad);
			for (int i = 0; i < 3; ++i) {
				a_lo[i] = mn[i] * a_k;
				a_hi[i] = mx[i] * a_k;
			}
		}

		bool Overlaps(const float* a_lo, const float* a_hi, const float* b_lo, const float* b_hi)
		{
			return a_lo[0] <= b_hi[0] && a_hi[0] >= b_lo[0] && a_lo[1] <= b_hi[1] && a_hi[1] >= b_lo[1] && a_lo[2] <= b_hi[2] && a_hi[2] >= b_lo[2];
		}

		const float* Vec(const void* a_base, std::size_t a_offset)
		{
			return reinterpret_cast<const float*>(reinterpret_cast<const std::uint8_t*>(a_base) + a_offset);
		}

		template <class T>
		T Field(const void* a_base, std::size_t a_offset)
		{
			T value;
			std::memcpy(&value, reinterpret_cast<const std::uint8_t*>(a_base) + a_offset, sizeof(T));
			return value;
		}

		// What you can stand on, run along and climb: the world, not things that move about.
		bool Included(RE::COL_LAYER a_layer)
		{
			switch (a_layer) {
			case RE::COL_LAYER::kStatic:
			case RE::COL_LAYER::kAnimStatic:
			case RE::COL_LAYER::kTransparent:
			case RE::COL_LAYER::kTrees:
			case RE::COL_LAYER::kProps:
			case RE::COL_LAYER::kTerrain:
			case RE::COL_LAYER::kGround:
			case RE::COL_LAYER::kInvisibleWall:
			// Also what level designers hang on the world for the player: collision boxes (over
			// rubble, along docks), stair ramps, small see-through things, and clutter (only what's
			// big enough to stand on, see Solid).
			case RE::COL_LAYER::kCollisionBox:
			case RE::COL_LAYER::kStairHelper:
			case RE::COL_LAYER::kTransparentSmall:
			case RE::COL_LAYER::kTransparentSmallAnim:
			case RE::COL_LAYER::kClutter:
				return true;
			default:
				return false;
			}
		}

		// Clutter only counts when it's something to stand on or climb (crates, carts, barrels), not
		// plates and cups to snag on: at least 28 units (0.4 m) every way and 50 (0.7 m) one way.
		bool Solid(RE::COL_LAYER a_layer, const float* a_lo, const float* a_hi)
		{
			if (a_layer != RE::COL_LAYER::kClutter) {
				return true;
			}
			const float x = a_hi[0] - a_lo[0], y = a_hi[1] - a_lo[1], z = a_hi[2] - a_lo[2];
			return std::min({ x, y, z }) >= 28.0f && std::max({ x, y, z }) >= 50.0f;
		}

		inline void Sub(const float* a, const float* b, float* o) { o[0] = a[0] - b[0], o[1] = a[1] - b[1], o[2] = a[2] - b[2]; }
		inline void Cross(const float* a, const float* b, float* o)
		{
			o[0] = a[1] * b[2] - a[2] * b[1];
			o[1] = a[2] * b[0] - a[0] * b[2];
			o[2] = a[0] * b[1] - a[1] * b[0];
		}
		inline float Dot(const float* a, const float* b) { return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]; }

		bool GuardedAabb(const RE::hkpShape* a_shape, const float* a_xf, RE::hkAabb& a_out)
		{
			__try {
				a_shape->GetAabbImpl(*reinterpret_cast<const RE::hkTransform*>(a_xf), 0.0f, a_out);
				return true;
			} __except (EXCEPTION_EXECUTE_HANDLER) {
				return false;
			}
		}

		void EmitAabbFallback(const RE::hkpShape* a_shape, const float* a_xf, float a_k, Job& a_job)
		{
			RE::hkAabb box;
			a_shape->GetAabbImpl(*reinterpret_cast<const RE::hkTransform*>(a_xf), 0.0f, box);
			float lo[3], hi[3];
			AabbToGame(box, a_k, lo, hi);
			if (!Finite(lo, 3) || !Finite(hi, 3) || hi[0] - lo[0] > 4000 || hi[1] - lo[1] > 4000 || hi[2] - lo[2] > 4000) {
				return;
			}
			Obb obb{};
			for (int i = 0; i < 3; ++i) {
				obb.c[i] = (lo[i] + hi[i]) * 0.5f;
				obb.half[i] = (hi[i] - lo[i]) * 0.5f;
				obb.axis[i][i] = 1.0f;
			}
			a_job.boxes.push_back(obb);
		}

		std::vector<int> loggedTypes;

		void Collect(const RE::hkpShape* a_shape, const float* a_xf, const float a_lo[3], const float a_hi[3], float a_k, Job& a_job, int a_depth)
		{
			if (!a_shape || a_depth > 8 || a_job.tris.size() > 4000000) {
				return;
			}
			using T = RE::hkpShapeType;
			const auto type = a_shape->type;

			switch (type) {
			case T::kMOPP:
			case T::kBVTree:
				{
					auto* bv = static_cast<const RE::hkpBvTreeShape*>(a_shape);
					// Query box -> Havok world -> shape-local (inverse transform of the 8 corners).
					const float hlo[3] = { a_lo[0] / a_k, a_lo[1] / a_k, a_lo[2] / a_k };
					const float hhi[3] = { a_hi[0] / a_k, a_hi[1] / a_k, a_hi[2] / a_k };
					float       llo[3] = { FLT_MAX, FLT_MAX, FLT_MAX }, lhi[3] = { -FLT_MAX, -FLT_MAX, -FLT_MAX };
					for (int c = 0; c < 8; ++c) {
						const float p[3] = { (c & 1) ? hhi[0] : hlo[0], (c & 2) ? hhi[1] : hlo[1], (c & 4) ? hhi[2] : hlo[2] };
						const float d[3] = { p[0] - a_xf[12], p[1] - a_xf[13], p[2] - a_xf[14] };
						for (int i = 0; i < 3; ++i) {
							const float v = a_xf[i * 4] * d[0] + a_xf[i * 4 + 1] * d[1] + a_xf[i * 4 + 2] * d[2];  // R^T d
							llo[i] = std::min(llo[i], v);
							lhi[i] = std::max(lhi[i], v);
						}
					}
					RE::hkAabb local;
					local.min = RE::hkVector4(llo[0], llo[1], llo[2], 0.0f);
					local.max = RE::hkVector4(lhi[0], lhi[1], lhi[2], 0.0f);
					thread_local std::vector<RE::hkpShapeKey> keys(kMaxKeys);  // per thread: read in the background too
					const auto  found = std::min<std::uint32_t>(bv->QueryAabbImpl(local, keys.data(), kMaxKeys), kMaxKeys);
					const auto* container = bv->GetContainer();
					if (!container) {
						return;
					}
					for (std::uint32_t i = 0; i < found; ++i) {
						RE::hkpShapeBuffer buffer;
						Collect(container->GetChildShape(keys[i], buffer), a_xf, a_lo, a_hi, a_k, a_job, a_depth + 1);
					}
					return;
				}
			case T::kList:
			case T::kCollection:
			case T::kCompressedMesh:
			case T::kExtendedMesh:
			case T::kTriangleCollection:
			case T::kConvexList:
				{
					const auto* container = a_shape->GetContainer();
					if (!container) {
						EmitAabbFallback(a_shape, a_xf, a_k, a_job);
						return;
					}
					int guard = 0;
					for (auto key = container->GetFirstKey(); key != RE::HK_INVALID_SHAPE_KEY && guard < 200000; key = container->GetNextKey(key), ++guard) {
						RE::hkpShapeBuffer buffer;
						const auto*        child = container->GetChildShape(key, buffer);
						if (!child) {
							continue;
						}
						RE::hkAabb box;
						child->GetAabbImpl(*reinterpret_cast<const RE::hkTransform*>(a_xf), 0.0f, box);
						float lo[3], hi[3];
						AabbToGame(box, a_k, lo, hi);
						if (Overlaps(lo, hi, a_lo, a_hi)) {
							Collect(child, a_xf, a_lo, a_hi, a_k, a_job, a_depth + 1);
						}
					}
					return;
				}
			case T::kTriangle:
				{
					Tri tri{};
					for (int v = 0; v < 3; ++v) {
						float w[3];
						XfPoint(a_xf, Vec(a_shape, 0x30 + v * 0x10), w);
						for (int i = 0; i < 3; ++i) {
							tri.v[v * 3 + i] = w[i] * a_k;
						}
					}
					if (Finite(tri.v, 9)) {
						a_job.tris.push_back(tri);
					}
					return;
				}
			case T::kBox:
				{
					const float* half = Vec(a_shape, 0x30);
					const float  radius = Field<float>(a_shape, 0x20);
					Obb          obb{};
					const float  zero[3] = { 0, 0, 0 };
					float        center[3];
					XfPoint(a_xf, zero, center);
					for (int i = 0; i < 3; ++i) {
						obb.c[i] = center[i] * a_k;
						for (int j = 0; j < 3; ++j) {
							obb.axis[i][j] = a_xf[i * 4 + j];
						}
						obb.half[i] = (half[i] + radius) * a_k;
					}
					if (Finite(obb.c, 3) && Finite(obb.half, 3)) {
						a_job.boxes.push_back(obb);
					}
					return;
				}
			case T::kCapsule:
			case T::kSphere:
				{
					const float radius = Field<float>(a_shape, 0x20);
					Capsule     cap{};
					float       w[3];
					const float zero[3] = { 0, 0, 0 };
					XfPoint(a_xf, type == T::kCapsule ? Vec(a_shape, 0x30) : zero, w);
					for (int i = 0; i < 3; ++i) {
						cap.a[i] = w[i] * a_k;
					}
					XfPoint(a_xf, type == T::kCapsule ? Vec(a_shape, 0x40) : zero, w);
					for (int i = 0; i < 3; ++i) {
						cap.b[i] = w[i] * a_k;
					}
					cap.r = radius * a_k;
					if (Finite(cap.a, 3) && Finite(cap.b, 3) && std::isfinite(cap.r) && cap.r < 4000.0f) {
						a_job.capsules.push_back(cap);
					}
					return;
				}
			case T::kConvexVertices:
				{
					const auto& planes = *reinterpret_cast<const RE::hkArray<RE::hkVector4>*>(reinterpret_cast<const std::uint8_t*>(a_shape) + 0x78);
					const float radius = Field<float>(a_shape, 0x20);
					if (planes.size() <= 0 || planes.size() > 512) {
						EmitAabbFallback(a_shape, a_xf, a_k, a_job);
						return;
					}
					Convex     cvx{};
					RE::hkAabb box;
					a_shape->GetAabbImpl(*reinterpret_cast<const RE::hkTransform*>(a_xf), 0.0f, box);
					AabbToGame(box, a_k, cvx.lo, cvx.hi);
					for (std::int32_t i = 0; i < planes.size(); ++i) {
						alignas(16) float p[4];
						_mm_store_ps(p, planes.data()[i].quad);
						float nw[3];
						XfDir(a_xf, p, nw);
						const float dw = p[3] - (nw[0] * a_xf[12] + nw[1] * a_xf[13] + nw[2] * a_xf[14]) - radius;
						cvx.planes.push_back({ nw[0], nw[1], nw[2], dw * a_k });
					}
					if (Finite(cvx.lo, 3) && Finite(cvx.hi, 3)) {
						a_job.convexes.push_back(std::move(cvx));
					}
					return;
				}
			case T::kConvexTransform:
			case T::kConvexTranslate:
				{
					const auto*       child = Field<const RE::hkpShape*>(a_shape, 0x30);
					alignas(16) float local[16] = { 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1 };
					if (type == T::kConvexTransform) {
						std::memcpy(local, Vec(a_shape, 0x40), sizeof(local));
					} else {
						std::memcpy(local + 12, Vec(a_shape, 0x40), sizeof(float) * 3);
					}
					if (!child || !XfLooksValid(local)) {
						EmitAabbFallback(a_shape, a_xf, a_k, a_job);
						return;
					}
					alignas(16) float composed[16];
					XfCompose(a_xf, local, composed);
					Collect(child, composed, a_lo, a_hi, a_k, a_job, a_depth + 1);
					return;
				}
			case T::kTransform:
				{
					const auto*       child = Field<const RE::hkpShape*>(a_shape, 0x28);
					alignas(16) float local[16];
					std::memcpy(local, Vec(a_shape, 0x50), sizeof(local));
					if (!child || !XfLooksValid(local)) {
						EmitAabbFallback(a_shape, a_xf, a_k, a_job);
						return;
					}
					alignas(16) float composed[16];
					XfCompose(a_xf, local, composed);
					Collect(child, composed, a_lo, a_hi, a_k, a_job, a_depth + 1);
					return;
				}
			default:
				if (std::ranges::find(loggedTypes, static_cast<int>(type)) == loggedTypes.end()) {
					loggedTypes.push_back(static_cast<int>(type));
					logger::info("collision: shape type {} taken as its bounding box (convex {})", static_cast<int>(type), a_shape->IsConvex());
				}
				if (a_shape->IsConvex()) {
					EmitAabbFallback(a_shape, a_xf, a_k, a_job);
				}
				return;
			}
		}

		using CollectFn = void (*)(const RE::hkpShape*, const float*, const float*, const float*, float, Job*);
		bool GuardedCollect(const RE::hkpShape* a_shape, const float* a_xf, const float* a_lo, const float* a_hi, float a_k, Job* a_job)
		{
			static constexpr CollectFn fn = [](const RE::hkpShape* s, const float* xf, const float* lo, const float* hi, float k, Job* job) {
				Collect(s, xf, lo, hi, k, *job, 0);
			};
			__try {
				fn(a_shape, a_xf, a_lo, a_hi, a_k, a_job);
				return true;
			} __except (EXCEPTION_EXECUTE_HANDLER) {
				return false;
			}
		}

		// The thin, long pieces that may be ziplines, swing poles or balance beams (faith_ffi
		// decides which): capsules at most 15 units thick and 70 long, and boxes thin across two
		// ways and long the third (their top's middle line, or their centre line when they're a
		// bar).
		void Candidates(const Job& a_src, std::vector<FaithFixtureCandidate>& a_out)
		{
			for (const auto& cap : a_src.capsules) {
				float ab[3];
				Sub(cap.b, cap.a, ab);
				if (cap.r <= 15.0f && Dot(ab, ab) >= 70.0f * 70.0f) {
					a_out.push_back({ { cap.a[0], cap.a[1], cap.a[2] }, { cap.b[0], cap.b[1], cap.b[2] }, cap.r, 1 });
				}
			}
			for (const auto& b : a_src.boxes) {
				int l = 0;
				for (int i = 1; i < 3; ++i) {
					if (b.half[i] > b.half[l]) {
						l = i;
					}
				}
				const int s0 = (l + 1) % 3, s1 = (l + 2) % 3;
				if (b.half[l] * 2.0f < 70.0f || b.half[s0] > 25.0f || b.half[s1] > 25.0f) {
					continue;
				}
				// Of the two thin ways, the one nearer vertical is its height.
				const int   up = std::fabs(b.axis[s0][2]) >= std::fabs(b.axis[s1][2]) ? s0 : s1;
				const int   across = up == s0 ? s1 : s0;
				const float sign = b.axis[up][2] >= 0.0f ? 1.0f : -1.0f;
				float       ta[3], tb[3], ca[3], cb[3];
				for (int k = 0; k < 3; ++k) {
					const float top = b.c[k] + b.axis[up][k] * b.half[up] * sign;
					ta[k] = top - b.axis[l][k] * b.half[l];
					tb[k] = top + b.axis[l][k] * b.half[l];
					ca[k] = b.c[k] - b.axis[l][k] * b.half[l];
					cb[k] = b.c[k] + b.axis[l][k] * b.half[l];
				}
				a_out.push_back({ { ta[0], ta[1], ta[2] }, { tb[0], tb[1], tb[2] }, b.half[across], 0 });
				if (b.half[s0] <= 8.0f && b.half[s1] <= 8.0f) {
					a_out.push_back({ { ca[0], ca[1], ca[2] }, { cb[0], cb[1], cb[2] }, std::max(b.half[s0], b.half[s1]), 1 });
				}
			}
		}

		// Boxes, capsules (as boxes) and convex hulls -> triangles, plus the job's own triangles.
		void Triangulate(const Job& a_src, std::vector<float>& a_out)
		{
			auto emit = [&](const float* a, const float* b, const float* c) {
				a_out.insert(a_out.end(), { a[0], a[1], a[2], b[0], b[1], b[2], c[0], c[1], c[2] });
			};
			for (const auto& t : a_src.tris) {
				a_out.insert(a_out.end(), std::begin(t.v), std::end(t.v));
			}
			auto quad = [&](const float* a, const float* b, const float* c, const float* d) {
				emit(a, b, c);
				emit(a, c, d);
			};
			auto box = [&](const float* c, const float (*axis)[3], const float* half) {
				float corner[8][3];
				for (int i = 0; i < 8; ++i) {
					const float sx = (i & 1) ? 1.0f : -1.0f, sy = (i & 2) ? 1.0f : -1.0f, sz = (i & 4) ? 1.0f : -1.0f;
					for (int k = 0; k < 3; ++k) {
						corner[i][k] = c[k] + axis[0][k] * half[0] * sx + axis[1][k] * half[1] * sy + axis[2][k] * half[2] * sz;
					}
				}
				quad(corner[0], corner[1], corner[3], corner[2]);
				quad(corner[4], corner[5], corner[7], corner[6]);
				quad(corner[0], corner[1], corner[5], corner[4]);
				quad(corner[2], corner[3], corner[7], corner[6]);
				quad(corner[0], corner[2], corner[6], corner[4]);
				quad(corner[1], corner[3], corner[7], corner[5]);
			};
			for (const auto& b : a_src.boxes) {
				box(b.c, b.axis, b.half);
			}
			for (const auto& cap : a_src.capsules) {
				float ab[3];
				Sub(cap.b, cap.a, ab);
				const float len = std::sqrt(Dot(ab, ab));
				float       axis[3][3]{};
				if (len > 1e-4f) {
					for (int k = 0; k < 3; ++k) {
						axis[2][k] = ab[k] / len;
					}
				} else {
					axis[2][2] = 1.0f;
				}
				const float ref[3] = { std::fabs(axis[2][2]) < 0.9f ? 0.0f : 1.0f, 0.0f, std::fabs(axis[2][2]) < 0.9f ? 1.0f : 0.0f };
				Cross(ref, axis[2], axis[0]);
				const float l0 = std::sqrt(Dot(axis[0], axis[0]));
				for (int k = 0; k < 3; ++k) {
					axis[0][k] /= l0;
				}
				Cross(axis[2], axis[0], axis[1]);
				const float c[3] = { (cap.a[0] + cap.b[0]) * 0.5f, (cap.a[1] + cap.b[1]) * 0.5f, (cap.a[2] + cap.b[2]) * 0.5f };
				const float half[3] = { cap.r, cap.r, len * 0.5f + cap.r };
				box(c, axis, half);
			}
			// Convex hull from planes: clip a big square on each plane by all the other planes.
			for (const auto& cvx : a_src.convexes) {
				const float ex = cvx.hi[0] - cvx.lo[0], ey = cvx.hi[1] - cvx.lo[1], ez = cvx.hi[2] - cvx.lo[2];
				const float diag = std::sqrt(ex * ex + ey * ey + ez * ez) + 1.0f;
				const float mid[3] = { (cvx.lo[0] + cvx.hi[0]) * 0.5f, (cvx.lo[1] + cvx.hi[1]) * 0.5f, (cvx.lo[2] + cvx.hi[2]) * 0.5f };
				for (std::size_t i = 0; i < cvx.planes.size(); ++i) {
					const auto& pl = cvx.planes[i];
					const float n[3] = { pl[0], pl[1], pl[2] };
					const float nl = std::sqrt(Dot(n, n));
					if (nl < 1e-6f) {
						continue;
					}
					const float dist = (Dot(n, mid) + pl[3]) / (nl * nl);
					const float o[3] = { mid[0] - n[0] * dist, mid[1] - n[1] * dist, mid[2] - n[2] * dist };
					const float ref[3] = { std::fabs(n[2]) < 0.9f * nl ? 0.0f : 1.0f, 0.0f, std::fabs(n[2]) < 0.9f * nl ? 1.0f : 0.0f };
					float       t1[3], t2[3];
					Cross(ref, n, t1);
					const float lt = std::sqrt(Dot(t1, t1));
					for (int k = 0; k < 3; ++k) {
						t1[k] /= lt;
					}
					Cross(n, t1, t2);
					const float l2 = std::sqrt(Dot(t2, t2));
					for (int k = 0; k < 3; ++k) {
						t2[k] /= l2;
					}
					std::vector<std::array<float, 3>> poly;
					const float                       sgn1[4] = { -1, 1, 1, -1 }, sgn2[4] = { -1, -1, 1, 1 };
					for (int q = 0; q < 4; ++q) {
						const float s1 = sgn1[q] * diag, s2 = sgn2[q] * diag;
						poly.push_back({ o[0] + t1[0] * s1 + t2[0] * s2, o[1] + t1[1] * s1 + t2[1] * s2, o[2] + t1[2] * s1 + t2[2] * s2 });
					}
					for (std::size_t j = 0; j < cvx.planes.size() && poly.size() >= 3; ++j) {
						if (j == i) {
							continue;
						}
						const auto&                       cp = cvx.planes[j];
						std::vector<std::array<float, 3>> out;
						for (std::size_t v = 0; v < poly.size(); ++v) {
							const auto& A = poly[v];
							const auto& B = poly[(v + 1) % poly.size()];
							const float da = cp[0] * A[0] + cp[1] * A[1] + cp[2] * A[2] + cp[3];
							const float db = cp[0] * B[0] + cp[1] * B[1] + cp[2] * B[2] + cp[3];
							if (da <= 0.0f) {
								out.push_back(A);
							}
							if ((da <= 0.0f) != (db <= 0.0f)) {
								const float t = da / (da - db);
								out.push_back({ A[0] + (B[0] - A[0]) * t, A[1] + (B[1] - A[1]) * t, A[2] + (B[2] - A[2]) * t });
							}
						}
						poly.swap(out);
					}
					for (std::size_t v = 1; v + 1 < poly.size(); ++v) {
						emit(poly[0].data(), poly[v].data(), poly[v + 1].data());
					}
				}
			}
		}
	}

	namespace
	{
		// Faith-only collision patches (Data\SKSE\Plugins\FaithParkour\<worldspace form ID>.bin,
		// made by the parkour tool from a survey): boxes added, and boxes whose collision is taken
		// away (centre, half size, yaw; game units).
		struct PatchBox
		{
			float c[3], h[3], yaw;
		};
		struct Patch
		{
			std::vector<PatchBox> add, remove;
		};
		std::unordered_map<RE::FormID, std::optional<Patch>> patches;

		std::mutex patchesLock;

		const Patch* PatchFor(RE::FormID a_world)
		{
			std::scoped_lock guard(patchesLock);
			auto it = patches.find(a_world);
			if (it == patches.end()) {
				std::optional<Patch> p;
				const auto path = std::format("Data/SKSE/Plugins/FaithParkour/{:08X}.bin", a_world);
				if (FILE* f = std::fopen(path.c_str(), "rb")) {
					char          magic[4]{};
					std::uint32_t nAdd = 0, nRemove = 0;
					if (std::fread(magic, 1, 4, f) == 4 && std::memcmp(magic, "FPK1", 4) == 0 && std::fread(&nAdd, 4, 1, f) == 1 && std::fread(&nRemove, 4, 1, f) == 1 &&
						nAdd < 100000 && nRemove < 100000) {
						Patch patch;
						patch.add.resize(nAdd);
						patch.remove.resize(nRemove);
						const bool ok = std::fread(patch.add.data(), sizeof(PatchBox), nAdd, f) == nAdd && std::fread(patch.remove.data(), sizeof(PatchBox), nRemove, f) == nRemove;
						if (ok) {
							logger::info("parkour patch {}: {} boxes added, {} taken away", path, nAdd, nRemove);
							p = std::move(patch);
						}
					}
					std::fclose(f);
				}
				it = patches.emplace(a_world, std::move(p)).first;
			}
			return it->second ? &*it->second : nullptr;
		}

		bool InBox(const PatchBox& a_b, float a_x, float a_y, float a_z)
		{
			const float dx = a_x - a_b.c[0], dy = a_y - a_b.c[1], dz = a_z - a_b.c[2];
			const float cs = std::cos(a_b.yaw), sn = std::sin(a_b.yaw);
			const float lx = dx * cs + dy * sn, ly = -dx * sn + dy * cs;
			return std::fabs(lx) <= a_b.h[0] && std::fabs(ly) <= a_b.h[1] && std::fabs(dz) <= a_b.h[2];
		}

		void ApplyPatch(const Patch& a_patch, const float* a_lo, const float* a_hi, std::vector<float>& a_tris)
		{
			if (!a_patch.remove.empty()) {
				std::vector<float> kept;
				kept.reserve(a_tris.size());
				for (std::size_t i = 0; i + 9 <= a_tris.size(); i += 9) {
					const float* t = &a_tris[i];
					const float  cx = (t[0] + t[3] + t[6]) / 3, cy = (t[1] + t[4] + t[7]) / 3, cz = (t[2] + t[5] + t[8]) / 3;
					if (std::ranges::none_of(a_patch.remove, [&](const PatchBox& b) { return InBox(b, cx, cy, cz); })) {
						kept.insert(kept.end(), t, t + 9);
					}
				}
				a_tris.swap(kept);
			}
			for (const auto& b : a_patch.add) {
				const float r = std::sqrt(b.h[0] * b.h[0] + b.h[1] * b.h[1]);
				if (b.c[0] + r < a_lo[0] || b.c[0] - r > a_hi[0] || b.c[1] + r < a_lo[1] || b.c[1] - r > a_hi[1] || b.c[2] + b.h[2] < a_lo[2] ||
					b.c[2] - b.h[2] > a_hi[2]) {
					continue;
				}
				const float cs = std::cos(b.yaw), sn = std::sin(b.yaw);
				float       corner[8][3];
				for (int i = 0; i < 8; ++i) {
					const float lx = (i & 1 ? 1.0f : -1.0f) * b.h[0], ly = (i & 2 ? 1.0f : -1.0f) * b.h[1], lz = (i & 4 ? 1.0f : -1.0f) * b.h[2];
					corner[i][0] = b.c[0] + lx * cs - ly * sn;
					corner[i][1] = b.c[1] + lx * sn + ly * cs;
					corner[i][2] = b.c[2] + lz;
				}
				static constexpr int kQuads[6][4] = { { 0, 1, 3, 2 }, { 4, 5, 7, 6 }, { 0, 1, 5, 4 }, { 2, 3, 7, 6 }, { 0, 2, 6, 4 }, { 1, 3, 7, 5 } };
				for (const auto& q : kQuads) {
						const int idx[6] = { q[0], q[1], q[2], q[0], q[2], q[3] };
					for (int k : idx) {
						a_tris.insert(a_tris.end(), { corner[k][0], corner[k][1], corner[k][2] });
					}
				}
			}
		}
	}

	namespace
	{
		// The bodies that can move (outside Havok's fixed island: doors, gates, drawbridges,
		// lifts, loose things), held so they're read again every frame (CollectMoving).
		std::vector<RE::hkpEntity*> moving;
		// Where each was when last read (and whether it was still in the world): unchanged, there's
		// nothing to read again.
		std::vector<std::array<float, 16>> movingAt;
		bool                               movingFresh = true;

		void ForgetMoving()
		{
			for (auto* e : moving) {
				e->RemoveReference();
			}
			moving.clear();
			movingAt.clear();
			movingFresh = true;
		}
	}

	namespace
	{
		// Read the collision of a_bhk's world around a_center (any thread: under the world's read
		// lock). The bodies that can move are held (a reference each) in a_moving, not read.
		bool HarvestOn(RE::bhkWorld* a_bhk, RE::FormID a_ws, const RE::NiPoint3& a_center, float a_radius, float a_up, float a_down, std::vector<float>& a_out,
			std::vector<FaithFixtureCandidate>* a_candidates, std::vector<RE::hkpEntity*>& a_moving)
	{
		a_out.clear();
		if (a_candidates) {
			a_candidates->clear();
		}
		auto* bhk = a_bhk;
		auto* world = bhk ? bhk->GetWorld1() : nullptr;
		if (!world) {
			return false;
		}
		const float k = RE::bhkWorld::GetWorldScaleInverse();  // Havok units -> game units
		const float lo[3] = { a_center.x - a_radius, a_center.y - a_radius, a_center.z - a_down };
		const float hi[3] = { a_center.x + a_radius, a_center.y + a_radius, a_center.z + a_up };
		Job         job;
		int         bodies = 0, faulted = 0;
		{
			RE::BSReadLockGuard lock(bhk->worldLock);
			bool fixed = true;
			auto addIsland = [&](RE::hkpSimulationIsland* a_island) {
				if (!a_island) {
					return;
				}
				auto& entities = a_island->entities;
				for (std::int32_t i = 0; i < entities.size(); ++i) {
					auto* entity = entities.data()[i];
					if (!entity) {
						continue;
					}
					const auto& collidable = entity->collidable;
					if (!Included(collidable.GetCollisionLayer())) {
						continue;
					}
					const auto* shape = collidable.shape;
					const auto* xf = static_cast<const float*>(collidable.motion);
					if (!shape || !xf || !Finite(xf, 16)) {
						continue;
					}
					RE::hkAabb box;
					if (!GuardedAabb(shape, xf, box)) {
						continue;
					}
					float blo[3], bhi[3];
					AabbToGame(box, k, blo, bhi);
					if (!Finite(blo, 3) || !Finite(bhi, 3) || !Overlaps(blo, bhi, lo, hi) || !Solid(collidable.GetCollisionLayer(), blo, bhi)) {
						continue;
					}
					++bodies;
					// What's animated (doors, gates, drawbridges: keyframed) is read where it is each
					// frame instead (CollectMoving). Loose physics (crates, barrels, carts) is read
					// with the rest: the player's capsule nudges it when Faith stands on it, and
					// following that every frame made the ground under her jitter (a landing, and its
					// step, every frame).
					if (!fixed && entity->motion.type.get() == RE::hkpMotion::MotionType::kKeyframed) {
						entity->AddReference();
						a_moving.push_back(entity);
						continue;
					}
					if (!GuardedCollect(shape, xf, lo, hi, k, &job)) {
						++faulted;
					}
				}
			};
			addIsland(world->fixedIsland);
			fixed = false;
			for (std::int32_t i = 0; i < world->activeSimulationIslands.size(); ++i) {
				addIsland(world->activeSimulationIslands.data()[i]);
			}
			for (std::int32_t i = 0; i < world->inactiveSimulationIslands.size(); ++i) {
				addIsland(world->inactiveSimulationIslands.data()[i]);
			}
		}
		Triangulate(job, a_out);
		if (a_candidates) {
			Candidates(job, *a_candidates);
		}
		if (a_ws) {
			if (const auto* patch = PatchFor(a_ws)) {
				ApplyPatch(*patch, lo, hi, a_out);
			}
		}
		static int logged = 0;
		if (logged < 5 || faulted) {
			++logged;
			logger::info("collision: {} bodies -> {} triangles ({} meshes, {} boxes, {} capsules, {} hulls){}", bodies, a_out.size() / 9, job.tris.size(),
				job.boxes.size(), job.capsules.size(), job.convexes.size(), faulted ? fmt::format(", {} faulted", faulted) : "");
		}
		return true;
	}

	}

	bool Harvest(const RE::NiPoint3& a_center, float a_radius, float a_up, float a_down, std::vector<float>& a_out, std::vector<FaithFixtureCandidate>* a_candidates)
	{
		auto* player = RE::PlayerCharacter::GetSingleton();
		auto* cell = player ? player->GetParentCell() : nullptr;
		auto* bhk = cell ? cell->GetbhkWorld() : nullptr;
		auto* ws = player ? player->GetWorldspace() : nullptr;
		std::vector<RE::hkpEntity*> found;
		const bool ok = HarvestOn(bhk, ws ? ws->GetFormID() : 0, a_center, a_radius, a_up, a_down, a_out, a_candidates, found);
		ForgetMoving();
		moving = std::move(found);
		movingFresh = true;
		return ok;
	}

	namespace
	{
		// One read in the background at a time; its result waits here for the main thread.
		std::mutex                         asyncLock;
		bool                               asyncBusy = false, asyncReady = false, asyncOk = false;
		std::vector<float>                 asyncTris;
		std::vector<FaithFixtureCandidate> asyncCandidates;
		std::vector<RE::hkpEntity*>        asyncMoving;
	}

	bool HarvestAsync(const RE::NiPoint3& a_center, float a_radius, float a_up, float a_down, bool a_candidates)
	{
		{
			std::scoped_lock guard(asyncLock);
			if (asyncBusy) {
				return false;
			}
			asyncBusy = true;
		}
		auto* player = RE::PlayerCharacter::GetSingleton();
		auto* cell = player ? player->GetParentCell() : nullptr;
		RE::NiPointer<RE::bhkWorld> bhk{ cell ? cell->GetbhkWorld() : nullptr };  // kept alive meanwhile
		auto*                       ws = player ? player->GetWorldspace() : nullptr;
		const RE::FormID            wsId = ws ? ws->GetFormID() : 0;
		if (!bhk) {
			std::scoped_lock guard(asyncLock);
			asyncBusy = false;
			return false;
		}
		std::thread([bhk, wsId, a_center, a_radius, a_up, a_down, a_candidates]() {
			std::vector<float>                 tris;
			std::vector<FaithFixtureCandidate> candidates;
			std::vector<RE::hkpEntity*>        found;
			const bool ok = HarvestOn(bhk.get(), wsId, a_center, a_radius, a_up, a_down, tris, a_candidates ? &candidates : nullptr, found);
			std::scoped_lock guard(asyncLock);
			for (auto* e : asyncMoving) {
				e->RemoveReference();  // an earlier result nobody took
			}
			asyncTris = std::move(tris);
			asyncCandidates = std::move(candidates);
			asyncMoving = std::move(found);
			asyncOk = ok;
			asyncReady = true;
			asyncBusy = false;
		}).detach();
		return true;
	}

	bool TakeHarvest(std::vector<float>& a_out, std::vector<FaithFixtureCandidate>& a_candidates)
	{
		std::scoped_lock guard(asyncLock);
		if (!asyncReady) {
			return false;
		}
		asyncReady = false;
		if (!asyncOk) {
			for (auto* e : asyncMoving) {
				e->RemoveReference();
			}
			asyncMoving.clear();
			return false;
		}
		a_out.swap(asyncTris);
		a_candidates.swap(asyncCandidates);
		ForgetMoving();
		moving = std::move(asyncMoving);
		asyncMoving.clear();
		movingFresh = true;
		return true;
	}

	std::string Survey(float a_radius, float a_height)
	{
		auto* player = RE::PlayerCharacter::GetSingleton();
		auto* ws = player ? player->GetWorldspace() : nullptr;
		if (!player) {
			return {};
		}
		const auto         at = player->GetPosition();
		std::vector<float> tris;
		// The raw collision (no patch): the tool makes the patch from it.
		auto* cell = player->GetParentCell();
		auto* bhk = cell ? cell->GetbhkWorld() : nullptr;
		auto* world = bhk ? bhk->GetWorld1() : nullptr;
		if (!world) {
			return {};
		}
		const float k = RE::bhkWorld::GetWorldScaleInverse();
		const float lo[3] = { at.x - a_radius, at.y - a_radius, at.z - a_height };
		const float hi[3] = { at.x + a_radius, at.y + a_radius, at.z + a_height };
		Job         job;
		{
			RE::BSReadLockGuard lock(bhk->worldLock);
			auto addIsland = [&](RE::hkpSimulationIsland* a_island) {
				if (!a_island) {
					return;
				}
				auto& entities = a_island->entities;
				for (std::int32_t i = 0; i < entities.size(); ++i) {
					auto* entity = entities.data()[i];
					if (!entity || !Included(entity->collidable.GetCollisionLayer())) {
						continue;
					}
					const auto* shape = entity->collidable.shape;
					const auto* xf = static_cast<const float*>(entity->collidable.motion);
					if (!shape || !xf || !Finite(xf, 16)) {
						continue;
					}
					RE::hkAabb box;
					if (!GuardedAabb(shape, xf, box)) {
						continue;
					}
					float blo[3], bhi[3];
					AabbToGame(box, k, blo, bhi);
					if (Finite(blo, 3) && Finite(bhi, 3) && Overlaps(blo, bhi, lo, hi) && Solid(entity->collidable.GetCollisionLayer(), blo, bhi)) {
						GuardedCollect(shape, xf, lo, hi, k, &job);
					}
				}
			};
			addIsland(world->fixedIsland);
			for (std::int32_t i = 0; i < world->activeSimulationIslands.size(); ++i) {
				addIsland(world->activeSimulationIslands.data()[i]);
			}
			for (std::int32_t i = 0; i < world->inactiveSimulationIslands.size(); ++i) {
				addIsland(world->inactiveSimulationIslands.data()[i]);
			}
		}
		Triangulate(job, tris);
		auto dir = SKSE::log::log_directory();
		if (!dir) {
			return {};
		}
		const RE::FormID id = ws ? ws->GetFormID() : 0;
		const auto       path = *dir / std::format("FaithSurvey_{:08X}.bin", id);
		FILE*            f = _wfopen(path.c_str(), L"wb");
		if (!f) {
			return {};
		}
		const char          magic[4] = { 'F', 'S', 'V', '1' };
		const std::uint32_t count = static_cast<std::uint32_t>(tris.size() / 9);
		const float         pos[3] = { at.x, at.y, at.z };
		std::fwrite(magic, 1, 4, f);
		std::fwrite(&id, 4, 1, f);
		std::fwrite(pos, 4, 3, f);
		std::fwrite(&count, 4, 1, f);
		std::fwrite(tris.data(), 4, tris.size(), f);
		std::fclose(f);
		logger::info("survey: {} triangles of worldspace {:08X} ({}) around ({:.0f} {:.0f} {:.0f}) -> {}", count, id, ws ? ws->GetFullName() : "?", at.x, at.y, at.z,
			path.string());
		return path.string();
	}

	bool GroundUnder(const std::vector<float>& a_tris, const RE::NiPoint3& a_at, float a_below)
	{
		for (std::size_t i = 0; i + 9 <= a_tris.size(); i += 9) {
			const float* t = &a_tris[i];
			// Facing up enough to stand on (either winding).
			const float ux = t[3] - t[0], uy = t[4] - t[1], uz = t[5] - t[2];
			const float vx = t[6] - t[0], vy = t[7] - t[1], vz = t[8] - t[2];
			const float nx = uy * vz - uz * vy, ny = uz * vx - ux * vz, nz = ux * vy - uy * vx;
			const float len = std::sqrt(nx * nx + ny * ny + nz * nz);
			if (len < 1e-6f || std::fabs(nz) < 0.5f * len) {
				continue;
			}
			// Under the point (barycentric in x, y), and below it within a_below.
			const float px = a_at.x - t[0], py = a_at.y - t[1];
			const float den = ux * vy - vx * uy;
			if (std::fabs(den) < 1e-6f) {
				continue;
			}
			const float b = (px * vy - vx * py) / den, c = (ux * py - px * uy) / den;
			if (b < 0.0f || c < 0.0f || b + c > 1.0f) {
				continue;
			}
			const float z = t[2] + b * uz + c * vz;
			if (z <= a_at.z + 60.0f && z >= a_at.z - a_below) {
				return true;
			}
		}
		return false;
	}

	namespace
	{
		// Skyrim's material -> Mirror's Edge's step sound set (faith_set_surfaces). Stone, and what
		// Mirror's Edge has no sound for (dirt, grass, snow, sand, mud), are concrete.
		std::uint32_t SurfaceOf(RE::MATERIAL_ID a_m)
		{
			using M = RE::MATERIAL_ID;
			switch (a_m) {
			case M::kWood:
			case M::kWoodLight:
			case M::kWoodHeavy:
			case M::kWoodStairs:
			case M::kWoodAsStairs:
			case M::kBarrel:
				return 1;
			case M::kMetalLight:
			case M::kMetalSolid:
			case M::kMetalHeavy:
			case M::kChainMetal:
			case M::kChain:
			case M::kPotsPans:
				return 2;
			case M::kBasket:
			case M::kBook:
			case M::kCarpet:
				return 8;
			case M::kWater:
				return 9;
			case M::kGlass:
			case M::kGlassStairs:
			case M::kIce:
			case M::kIceForm:
				return 10;
			default:
				return 0;
			}
		}
	}

	namespace
	{
		// The material where a ray hit: a mesh in a MOPP tree knows it per triangle (its own
		// wrapper, not the tree's: the tree's has no table and reading one crashes); anything
		// else is one material.
		std::uint32_t MaterialUnguarded(const RE::hkpShape* a_shape, RE::hkpShapeKey a_key)
		{
			using T = RE::hkpShapeType;
			const RE::hkpShape* leaf = a_shape;
			if (a_shape->type == T::kMOPP) {
				leaf = static_cast<const RE::hkpMoppBvTreeShape*>(a_shape)->child.childShape;
			}
			auto* wrapper = leaf ? leaf->userData : nullptr;
			if (!wrapper) {
				wrapper = a_shape->userData;
			}
			if (!wrapper) {
				return 0;
			}
			if (leaf && leaf->type == T::kCompressedMesh && a_key != RE::HK_INVALID_SHAPE_KEY) {
				return SurfaceOf(wrapper->GetMaterialID(a_key));
			}
			return SurfaceOf(wrapper->materialID);
		}

		// Guarded: odd collision data gives concrete steps, never a crash.
		std::uint32_t GuardedMaterial(const RE::hkpShape* a_shape, RE::hkpShapeKey a_key)
		{
			__try {
				return MaterialUnguarded(a_shape, a_key);
			} __except (EXCEPTION_EXECUTE_HANDLER) {
				return 0;
			}
		}
	}

	std::uint32_t SurfaceAt(const RE::NiPoint3& a_from, const RE::NiPoint3& a_to)
	{
		auto* player = RE::PlayerCharacter::GetSingleton();
		auto* cell = player ? player->GetParentCell() : nullptr;
		auto* bhk = cell ? cell->GetbhkWorld() : nullptr;
		if (!bhk) {
			return 0;
		}
		const float   s = RE::bhkWorld::GetWorldScale();
		RE::bhkPickData pick{};
		pick.rayInput.from = RE::hkVector4(a_from.x * s, a_from.y * s, a_from.z * s, 0.0f);
		pick.rayInput.to = RE::hkVector4(a_to.x * s, a_to.y * s, a_to.z * s, 0.0f);
		pick.rayInput.enableShapeCollectionFilter = true;
		pick.rayInput.filterInfo.SetCollisionLayer(RE::COL_LAYER::kLOS);
		if (!bhk->PickObject(pick) || !pick.rayOutput.HasHit()) {
			return 0;
		}
		const auto* shape = pick.rayOutput.rootCollidable ? pick.rayOutput.rootCollidable->GetShape() : nullptr;
		return shape ? GuardedMaterial(shape, pick.rayOutput.shapeKeys[0]) : 0;
	}

	bool CollectMoving(std::vector<float>& a_out)
	{
		a_out.clear();
		if (moving.empty()) {
			return false;
		}
		auto* player = RE::PlayerCharacter::GetSingleton();
		auto* cell = player ? player->GetParentCell() : nullptr;
		auto* bhk = cell ? cell->GetbhkWorld() : nullptr;
		if (!bhk) {
			return false;
		}
		const float k = RE::bhkWorld::GetWorldScaleInverse();
		const float lo[3] = { -1e9f, -1e9f, -1e9f }, hi[3] = { 1e9f, 1e9f, 1e9f };
		Job         job;
		{
			RE::BSReadLockGuard lock(bhk->worldLock);
			// Has anything moved (or left the world) since the last read?
			bool changed = movingFresh || movingAt.size() != moving.size();
			movingAt.resize(moving.size());
			for (std::size_t i = 0; i < moving.size(); ++i) {
				std::array<float, 16> now{};
				const auto*           xf = moving[i]->world ? static_cast<const float*>(moving[i]->collidable.motion) : nullptr;
				if (xf) {
					std::memcpy(now.data(), xf, sizeof(now));
				} else {
					now.fill(std::numeric_limits<float>::quiet_NaN());
				}
				for (int c = 0; c < 16 && !changed; ++c) {
					const float a = now[c], b = movingAt[i][c];
					changed = !(a == b || (std::isnan(a) && std::isnan(b)) || std::fabs(a - b) < 1e-4f);
				}
				movingAt[i] = now;
			}
			movingFresh = false;
			if (!changed) {
				return false;
			}
			for (auto* e : moving) {
				// Taken out of the world since (its cell unloaded, it was picked up): skip it.
				if (!e->world) {
					continue;
				}
				const auto* shape = e->collidable.shape;
				const auto* xf = static_cast<const float*>(e->collidable.motion);
				if (shape && xf && Finite(xf, 16)) {
					GuardedCollect(shape, xf, lo, hi, k, &job);
				}
			}
		}
		Triangulate(job, a_out);
		return true;
	}

	void Forget()
	{
		ForgetMoving();
	}
}
