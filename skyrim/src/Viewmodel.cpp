// Faith's own first-person body, drawn into Skyrim's frame.
//
// Mirror's Edge's arms and torso (SK_UpperBody) and legs (SK_LowerBody), skinned each frame by
// faith_ffi exactly as the Faith Runner app skins them (forearm twist morphs included), drawn
// into Skyrim's HDR scene right after Main::RenderWorld: before its tone mapping, bloom and
// grading, so they finish the same way as the world. Lit like Skyrim lights its own surfaces:
// the sun (or the interior's directional light), the directional ambient, and the nearest
// point lights. The way the light is gathered follows SkyCraft's WorldRender.cpp (MIT License,
// Copyright (c) 2026 chasmlol).
//
// Like Mirror's Edge's first-person body (SDPG_Foreground, Model1pFOV 100), it has its own
// depth buffer and field of view, so it never clips into walls; looking down, the field of
// view blends to the world's so the legs meet the ground.
#include "Viewmodel.h"

#include "Config.h"

#include <d3d11.h>
#include <d3dcompiler.h>

namespace faith::Viewmodel
{
	namespace
	{
		constexpr char kShader[] = R"(
cbuffer Frame : register(b0)
{
	row_major float4x4 proj;
	float4 sunDir;        // camera space, towards the light
	float4 sunColor;      // rgb; w: point light count
	float4 ambient[6];    // Skyrim's directional ambient, faces looking +X, -X, +Y, -Y, +Z, -Z (world)
	float4 camRight;      // world axes of camera space x, y, z
	float4 camUp;
	float4 camBack;
	float4 lightPos[8];   // camera space; w: 1/radius
	float4 lightColor[8];
	row_major float4x4 worldViewProj;     // Skyrim's camera: camera-relative world -> clip
	row_major float4x4 skyShadowProj[2];  // Skyrim's sun shadow cascades: camera-relative world -> shadow map uv, depth
	float4 skyShadowSplits;               // cascade 0 end, cascade 1 end (view depth), -, cascade count
	float4 skyShadowParams;               // slice of cascade 0, slice of cascade 1, texel size (uv), +1 standard / -1 reversed
	float4 heldDepth;                     // Skyrim's near, far, reversed, on: what's in her hands goes in front
	float4 heldScale;                     // this target's pixels -> Skyrim's depth texels (x, y); how close counts as held
	row_major float4x4 prevWorldViewProj; // last frame's camera: this frame's camera-relative world -> its clip
};
cbuffer Object : register(b1)
{
	float4 maps;          // x: normal map, y: specular map
	float4 mode;          // x: drawn with Skyrim's camera, into its depth (the legs);
	                      // w: stands still in the world (the course): its motion for Skyrim's TAA
};
Texture2D colourMap : register(t0);
Texture2D normalMap : register(t1);
Texture2D specMap : register(t2);
Texture2DArray<float> skyrimShadowMaps : register(t3);
Texture2D<float> skyrimDepth : register(t4);
SamplerState linearSampler : register(s0);
SamplerState pointSampler : register(s1);

struct VSIn
{
	float3 pos : POSITION;
	float3 normal : NORMAL;
	float4 tangent : TANGENT;
	float2 uv : TEXCOORD0;
};
struct VSOut
{
	float4 pos : SV_Position;
	float3 view : TEXCOORD0;
	float3 normal : TEXCOORD1;
	float4 tangent : TEXCOORD2;
	float2 uv : TEXCOORD3;
	float3 rel : TEXCOORD4;   // camera-relative world position
	float4 clipNow : TEXCOORD5;
	float4 clipPrev : TEXCOORD6;
};
struct PSOut
{
	float4 color : SV_Target0;
	float4 motion : SV_Target1;
};

VSOut VS(VSIn i)
{
	VSOut o;
	o.rel = i.pos.x * camRight.xyz + i.pos.y * camUp.xyz + i.pos.z * camBack.xyz;
	o.pos = mode.x > 0.5 ? mul(worldViewProj, float4(o.rel, 1.0)) : mul(proj, float4(i.pos, 1.0));
	o.view = i.pos;
	o.normal = i.normal;
	o.tangent = i.tangent;
	o.uv = i.uv;
	o.clipNow = o.pos;
	o.clipPrev = mode.w > 0.5 ? mul(prevWorldViewProj, float4(o.rel, 1.0)) : o.pos;
	return o;
}

float3 Ambient(float3 n)
{
	float3 n2 = n * n;
	return n2.x * (n.x >= 0 ? ambient[0].rgb : ambient[1].rgb)
	     + n2.y * (n.y >= 0 ? ambient[2].rgb : ambient[3].rgb)
	     + n2.z * (n.z >= 0 ? ambient[4].rgb : ambient[5].rgb);
}

// How much sun reaches a camera-relative point past Skyrim's own geometry (trees, buildings,
// terrain), from the shadow maps Skyrim rendered for the sun this frame: 1 lit, 0 shadowed.
// (SkyCraft's SkyrimSunShadow.)
float SkyrimSunShadow(float3 rel, float3 n, float viewZ)
{
	if (skyShadowSplits.w < 0.5) {
		return 1.0;
	}
	uint cascade = viewZ < skyShadowSplits.x ? 0 : 1;
	if (cascade >= (uint)skyShadowSplits.w || viewZ >= (cascade == 0 ? skyShadowSplits.x : skyShadowSplits.y)) {
		return 1.0;
	}
	float4 ls = mul(skyShadowProj[cascade], float4(rel + n * 3.0, 1.0));
	ls.xyz /= ls.w;
	if (any(ls.xy <= 0.0) || any(ls.xy >= 1.0)) {
		return 1.0;
	}
	float slice = cascade == 0 ? skyShadowParams.x : skyShadowParams.y;
	float lit = 0.0;
	[unroll] for (int y = -1; y <= 1; ++y) {
		[unroll] for (int x = -1; x <= 1; ++x) {
			float d = skyrimShadowMaps.SampleLevel(pointSampler, float3(ls.xy + float2(x, y) * skyShadowParams.z, slice), 0);
			lit += skyShadowParams.w > 0.0 ? (ls.z - 0.0005 <= d ? 1.0 : 0.0) : (ls.z + 0.0005 >= d ? 1.0 : 0.0);
		}
	}
	return lit / 9.0;
}

PSOut PS(VSOut i, bool front : SV_IsFrontFace)
{
	// Her arms are drawn over everything (their own depth), except what Skyrim drew right in
	// front of her: the weapon, shield or spell in her hand (Skyrim's first-person objects, posed
	// into her grip), so her fingers close round it rather than over it.
	if (mode.x < 0.5 && heldDepth.w > 0.5) {
		float d = skyrimDepth.Load(int3(i.pos.xy * heldScale.xy, 0));
		float n = heldDepth.x, f = heldDepth.y;
		float z = heldDepth.z > 0.5 ? f * n / (n + d * (f - n)) : f * n / max(f - d * (f - n), 1e-4);
		if (z < heldScale.z && z < -i.view.z - 0.5) {
			discard;
		}
	}
	float3 n = normalize(i.normal) * (front ? 1.0 : -1.0);
	if (maps.x > 0.5) {
		float3 t = normalize(i.tangent.xyz - n * dot(i.tangent.xyz, n));
		float3 b = i.tangent.w * cross(n, t);
		float3 m = normalMap.Sample(linearSampler, i.uv).xyz * 2.0 - 1.0;
		n = normalize(t * m.x + b * m.y + n * max(m.z, 0.05));
	}
	float4 albedo = colourMap.Sample(linearSampler, i.uv);
	float3 world = n.x * camRight.xyz + n.y * camUp.xyz + n.z * camBack.xyz;
	float  sunLit = SkyrimSunShadow(i.rel, world, -i.view.z);
	float3 lit = Ambient(world) + sunColor.rgb * saturate(dot(n, sunDir.xyz)) * sunLit;
	float3 v = normalize(-i.view);
	float  specAmount = maps.y > 0.5 ? dot(specMap.Sample(linearSampler, i.uv).rgb, float3(0.299, 0.587, 0.114)) : 0.15;
	float3 spec = sunColor.rgb * pow(saturate(dot(n, normalize(sunDir.xyz + v))), 24.0) * specAmount * 0.35 * (dot(n, sunDir.xyz) > 0 ? sunLit : 0.0);
	uint count = (uint)sunColor.w;
	for (uint k = 0; k < count; ++k) {
		float3 d = lightPos[k].xyz - i.view;
		float  f = saturate(length(d) * lightPos[k].w);
		lit += lightColor[k].rgb * ((1.0 - f * f) * saturate(dot(n, normalize(d))));
	}
	PSOut o;
	if (mode.y > 0.5) {
		// Drawn after Skyrim's own tone mapping (before the HUD): mapped to the screen here, the
		// way SkyCraft maps its blocks when it draws on the finished frame.
		o.color = float4(albedo.rgb * (1.0 - exp(-max(lit, 0.0) * 1.6)) + (1.0 - exp(-spec * 1.6)), 1.0);
	} else {
		o.color = float4(albedo.rgb * max(lit, 0.0) + spec, 1.0);
	}
	// Her body stays put on screen: Skyrim's TAA shouldn't drag it. The course stands still in
	// the world: it moved on screen as the camera did, which TAA needs to know or it smears the
	// last frame over this one (Skyrim's convention: (-0.5, 0.5) x (now - before), in NDC).
	float2 now = i.clipNow.xy / i.clipNow.w, before = i.clipPrev.xy / max(abs(i.clipPrev.w), 1e-6) * sign(i.clipPrev.w);
	o.motion = mode.w > 0.5 ? float4(float2(-0.5, 0.5) * (now - before), 0.0, 1.0) : float4(0.0, 0.0, 0.0, 1.0);
	return o;
}
)";

		// Mirror's Edge's speed blur, TdMotionBlurShader.usf's MainPixelShader as shipped: 8
		// samples, weights falling 1 to 1/8, strength clamp(dist^0.1 - 0.95, 0, 0.07) x
		// MotionPacked.r. Its "float offset = DestinationScreenVector * amount / 8" keeps only
		// the vector's x (an implicit float2 -> float truncation), and that one value steps both
		// u and v: so it is, exactly as in the game.
		constexpr char kBlurShader[] = R"(
cbuffer Blur : register(b0)
{
	float4 rect;      // the scene's viewport in the target: x, y offset; z, w size (uv)
	float4 clampUv;   // RenderTargetClampParameter: min u, v, max u, v
	float4 motion;    // x: MotionPacked.r
};
Texture2D scene : register(t0);
SamplerState linearSampler : register(s0);

struct VSOut
{
	float4 pos : SV_Position;
	float2 screen : TEXCOORD0;   // 0-1 across the scene's viewport
};

VSOut VS(uint id : SV_VertexID)
{
	VSOut o;
	float2 t = float2((id << 1) & 2, id & 2);
	o.pos = float4(t * float2(2, -2) + float2(-1, 1), 0, 1);
	o.screen = t;
	return o;
}

float4 PS(VSOut i) : SV_Target
{
	float2 destination = (-0.5 + float2(1 - i.screen.y, i.screen.x)) * 2;
	destination.y = -destination.y;
	float  dist = length(destination);
	destination = destination / max(dist, 1e-6);
	dist = clamp(exp2(0.1 * log2(max(dist, 1e-6))) - 0.95, 0, 0.07);
	float amount = dist * motion.x;
	float2 uv = rect.xy + i.screen * rect.zw;
	float  offset = destination.x * amount * 0.125;
	float3 result = 0;
	float  weight = 1.0;
	float3 centre = scene.SampleLevel(linearSampler, clamp(uv, clampUv.xy, clampUv.zw), 0).rgb;
	centre = all(isfinite(centre)) ? max(centre, 0.0) : 0.0;
	[unroll] for (int k = 0; k < 8; k++) {
		// A stray NaN or infinity in Skyrim's HDR scene, smeared over the blur, would reach its
		// eye adaptation (the whole screen's brightness) and black out the frame: never let one
		// through.
		float3 s = scene.SampleLevel(linearSampler, clamp(uv, clampUv.xy, clampUv.zw), 0).rgb;
		result += (all(isfinite(s)) ? max(s, 0.0) : centre) * weight;
		uv += offset;
		weight -= 0.125;
	}
	return float4(min(result * 0.222222, 65000.0), 1);
}
)";

		struct BlurConstants
		{
			float rect[4];
			float clampUv[4];
			float motion[4];
		};
		ID3D11VertexShader*       blurVs = nullptr;
		ID3D11PixelShader*        blurPs = nullptr;
		ID3D11Buffer*             blurCb = nullptr;
		ID3D11SamplerState*       blurSampler = nullptr;
		ID3D11Texture2D*          sceneCopy = nullptr;
		ID3D11ShaderResourceView* sceneCopySrv = nullptr;
		ID3D11BlendState*         blurBlend = nullptr;  // colour only: Skyrim keeps its alpha
		bool                      blurFailed = false;

		struct FrameConstants
		{
			float proj[4][4];
			float sunDir[4];
			float sunColor[4];
			float ambient[6][4];
			float camRight[4];
			float camUp[4];
			float camBack[4];
			float lightPos[8][4];
			float lightColor[8][4];
			float worldViewProj[4][4];
			float skyShadowProj[2][4][4];
			float skyShadowSplits[4];
			float skyShadowParams[4];
			float heldDepth[4];
			float heldScale[4];
			float prevWorldViewProj[4][4];
		};
		struct ObjectConstants
		{
			float maps[4];
			float mode[4];
		};

		struct Material
		{
			ID3D11ShaderResourceView* colour = nullptr;
			ID3D11ShaderResourceView* normal = nullptr;
			ID3D11ShaderResourceView* spec = nullptr;
		};
		struct Part
		{
			FaithPartInfo             info{};
			std::vector<FaithSection> sections;
			std::vector<Material>     materials;
			ID3D11Buffer*             vb = nullptr;
			ID3D11Buffer*             ib = nullptr;
			std::vector<FaithVertex>  verts;
		};

		bool                      ready = false, failed = false, visible = true;
		ID3D11Device*             device = nullptr;
		ID3D11VertexShader*       vs = nullptr;
		ID3D11PixelShader*        ps = nullptr;
		ID3D11InputLayout*        layout = nullptr;
		ID3D11Buffer*             frameCb = nullptr;
		ID3D11Buffer*             objectCb = nullptr;
		ID3D11SamplerState*       sampler = nullptr;
		ID3D11SamplerState*       pointSampler = nullptr;
		ID3D11DepthStencilState*  depthStateRev = nullptr;  // into Skyrim's depth when it's reversed
		// Skyrim's sun shadow cascades, copied when the sun's shadow pass ends (other lights reuse
		// the shared shadow map after it).
		ID3D11Texture2D*          sunShadowCopy = nullptr;
		ID3D11ShaderResourceView* sunShadowCopySrv = nullptr;
		ID3D11ShaderResourceView* noShadowSrv = nullptr;
		struct Cascade
		{
			float         m[4][4];
			float         split;
			std::uint32_t slice;
		};
		Cascade                   cascades[2]{};
		std::uint32_t             cascadeCount = 0;
		std::chrono::steady_clock::time_point shadowCaptureTime{};
		ID3D11RasterizerState*    raster = nullptr;
		ID3D11DepthStencilState*  depthState = nullptr;
		ID3D11BlendState*         blend = nullptr;
		ID3D11ShaderResourceView* white = nullptr;
		ID3D11Texture2D*          depthTex = nullptr;
		ID3D11DepthStencilView*   dsv = nullptr;
		UINT                      depthW = 0, depthH = 0;
		std::vector<Part>         parts;

		// The training course: the app's grid on each look's colour, and its triangles this frame.
		ID3D11ShaderResourceView*      courseLooks[7]{};
		ID3D11Buffer*                  courseVb = nullptr;
		UINT                           courseVbSize = 0;
		std::vector<FaithCourseVertex> courseSrc;
		std::vector<FaithVertex>       courseVerts;

		template <class T>
		void Release(T*& a_p)
		{
			if (a_p) {
				a_p->Release();
				a_p = nullptr;
			}
		}

		ID3D11ShaderResourceView* MakeTexture(const std::uint8_t* a_rgba, std::uint32_t a_w, std::uint32_t a_h)
		{
			D3D11_TEXTURE2D_DESC td{};
			td.Width = a_w;
			td.Height = a_h;
			td.MipLevels = 0;
			td.ArraySize = 1;
			td.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
			td.SampleDesc.Count = 1;
			td.Usage = D3D11_USAGE_DEFAULT;
			td.BindFlags = D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_RENDER_TARGET;
			td.MiscFlags = D3D11_RESOURCE_MISC_GENERATE_MIPS;
			ID3D11Texture2D* tex = nullptr;
			if (FAILED(device->CreateTexture2D(&td, nullptr, &tex))) {
				return nullptr;
			}
			ID3D11DeviceContext* ctx = nullptr;
			device->GetImmediateContext(&ctx);
			ctx->UpdateSubresource(tex, 0, nullptr, a_rgba, a_w * 4, 0);
			ID3D11ShaderResourceView* srv = nullptr;
			device->CreateShaderResourceView(tex, nullptr, &srv);
			if (srv) {
				ctx->GenerateMips(srv);
			}
			ctx->Release();
			tex->Release();
			return srv;
		}

		bool Compile(const char* a_entry, const char* a_target, ID3DBlob** a_out)
		{
			ID3DBlob*  errors = nullptr;
			const auto hr = D3DCompile(kShader, sizeof(kShader) - 1, "faith_viewmodel", nullptr, nullptr, a_entry, a_target, D3DCOMPILE_OPTIMIZATION_LEVEL3, 0, a_out, &errors);
			if (FAILED(hr)) {
				logger::error("viewmodel shader {}: {}", a_entry, errors ? static_cast<const char*>(errors->GetBufferPointer()) : "?");
				Release(errors);
				return false;
			}
			Release(errors);
			return true;
		}

		bool Init(::Faith* a_faith, ID3D11Device* a_device)
		{
			device = a_device;
			ID3DBlob *vsb = nullptr, *psb = nullptr;
			if (!Compile("VS", "vs_5_0", &vsb) || !Compile("PS", "ps_5_0", &psb)) {
				Release(vsb);
				return false;
			}
			device->CreateVertexShader(vsb->GetBufferPointer(), vsb->GetBufferSize(), nullptr, &vs);
			device->CreatePixelShader(psb->GetBufferPointer(), psb->GetBufferSize(), nullptr, &ps);
			const D3D11_INPUT_ELEMENT_DESC elems[] = {
				{ "POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0 },
				{ "NORMAL", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 12, D3D11_INPUT_PER_VERTEX_DATA, 0 },
				{ "TANGENT", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 24, D3D11_INPUT_PER_VERTEX_DATA, 0 },
				{ "TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 40, D3D11_INPUT_PER_VERTEX_DATA, 0 },
			};
			device->CreateInputLayout(elems, 4, vsb->GetBufferPointer(), vsb->GetBufferSize(), &layout);
			Release(vsb);
			Release(psb);

			D3D11_BUFFER_DESC cb{};
			cb.Usage = D3D11_USAGE_DYNAMIC;
			cb.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
			cb.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;
			cb.ByteWidth = sizeof(FrameConstants);
			device->CreateBuffer(&cb, nullptr, &frameCb);
			cb.ByteWidth = sizeof(ObjectConstants);
			device->CreateBuffer(&cb, nullptr, &objectCb);

			D3D11_SAMPLER_DESC sd{};
			sd.Filter = D3D11_FILTER_ANISOTROPIC;
			sd.MaxAnisotropy = 8;
			sd.AddressU = sd.AddressV = sd.AddressW = D3D11_TEXTURE_ADDRESS_WRAP;
			sd.MaxLOD = D3D11_FLOAT32_MAX;
			device->CreateSamplerState(&sd, &sampler);
			D3D11_SAMPLER_DESC pd{};
			pd.Filter = D3D11_FILTER_MIN_MAG_MIP_POINT;
			pd.AddressU = pd.AddressV = pd.AddressW = D3D11_TEXTURE_ADDRESS_CLAMP;
			pd.MaxLOD = D3D11_FLOAT32_MAX;
			device->CreateSamplerState(&pd, &pointSampler);

			D3D11_RASTERIZER_DESC rd{};
			rd.FillMode = D3D11_FILL_SOLID;
			rd.CullMode = D3D11_CULL_NONE;  // the open waist and cuffs show their insides, as in the app
			rd.FrontCounterClockwise = TRUE;
			rd.DepthClipEnable = TRUE;
			device->CreateRasterizerState(&rd, &raster);

			D3D11_DEPTH_STENCIL_DESC dd{};
			dd.DepthEnable = TRUE;
			dd.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ALL;
			dd.DepthFunc = D3D11_COMPARISON_LESS;
			device->CreateDepthStencilState(&dd, &depthState);
			dd.DepthFunc = D3D11_COMPARISON_GREATER;
			device->CreateDepthStencilState(&dd, &depthStateRev);
			{
				// A 1x1 shadow map array for when there are no cascades (never sampled then).
				D3D11_TEXTURE2D_DESC td{};
				td.Width = td.Height = 1;
				td.MipLevels = 1;
				td.ArraySize = 2;
				td.Format = DXGI_FORMAT_R32_FLOAT;
				td.SampleDesc.Count = 1;
				td.BindFlags = D3D11_BIND_SHADER_RESOURCE;
				const float           one[2] = { 1.0f, 1.0f };
				D3D11_SUBRESOURCE_DATA init[2]{ { &one[0], 4, 4 }, { &one[1], 4, 4 } };
				ID3D11Texture2D*      tex = nullptr;
				if (SUCCEEDED(device->CreateTexture2D(&td, init, &tex))) {
					device->CreateShaderResourceView(tex, nullptr, &noShadowSrv);
					tex->Release();
				}
			}

			D3D11_BLEND_DESC bd{};
			for (auto& t : bd.RenderTarget) {
				t.RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL;
			}
			device->CreateBlendState(&bd, &blend);

			const std::uint8_t px[4] = { 255, 255, 255, 255 };
			white = MakeTexture(px, 1, 1);

			const auto count = faith_body_parts(a_faith);
			for (std::uint32_t p = 0; p < count; ++p) {
				Part part;
				if (!faith_body_part(a_faith, p, &part.info)) {
					continue;
				}
				const auto* secs = faith_body_sections(a_faith, p);
				part.sections.assign(secs, secs + part.info.section_count);
				part.materials.resize(part.info.material_count);
				for (std::uint32_t m = 0; m < part.info.material_count; ++m) {
					for (std::uint32_t kind = 0; kind < 3; ++kind) {
						std::uint32_t w = 0, h = 0;
						const auto*   rgba = faith_body_texture(a_faith, p, m, kind, &w, &h);
						auto*         srv = rgba && w && h ? MakeTexture(rgba, w, h) : nullptr;
						(kind == 0 ? part.materials[m].colour : kind == 1 ? part.materials[m].normal : part.materials[m].spec) = srv;
					}
					logger::info("viewmodel: part {} material {} '{}': colour {}, normal map {}, specular {}", p, m, faith_body_material_name(a_faith, p, m),
						part.materials[m].colour != nullptr, part.materials[m].normal != nullptr, part.materials[m].spec != nullptr);
				}
				D3D11_BUFFER_DESC vbd{};
				vbd.Usage = D3D11_USAGE_DYNAMIC;
				vbd.BindFlags = D3D11_BIND_VERTEX_BUFFER;
				vbd.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;
				vbd.ByteWidth = part.info.vertex_count * sizeof(FaithVertex);
				device->CreateBuffer(&vbd, nullptr, &part.vb);
				D3D11_BUFFER_DESC ibd{};
				ibd.Usage = D3D11_USAGE_IMMUTABLE;
				ibd.BindFlags = D3D11_BIND_INDEX_BUFFER;
				ibd.ByteWidth = part.info.index_count * sizeof(std::uint32_t);
				D3D11_SUBRESOURCE_DATA init{ faith_body_indices(a_faith, p), 0, 0 };
				device->CreateBuffer(&ibd, &init, &part.ib);
				part.verts.resize(part.info.vertex_count);
				logger::info("viewmodel: part {} ({}): {} vertices, {} triangles, {} sections", p, part.info.legs ? "legs" : "arms and torso",
					part.info.vertex_count, part.info.index_count / 3, part.info.section_count);
				parts.push_back(std::move(part));
			}
			return vs && ps && layout && frameCb && objectCb && sampler && raster && depthState && blend && !parts.empty();
		}

		bool EnsureDepth(UINT a_w, UINT a_h)
		{
			if (dsv && depthW == a_w && depthH == a_h) {
				return true;
			}
			Release(dsv);
			Release(depthTex);
			D3D11_TEXTURE2D_DESC td{};
			td.Width = a_w;
			td.Height = a_h;
			td.MipLevels = 1;
			td.ArraySize = 1;
			td.Format = DXGI_FORMAT_D32_FLOAT;
			td.SampleDesc.Count = 1;
			td.Usage = D3D11_USAGE_DEFAULT;
			td.BindFlags = D3D11_BIND_DEPTH_STENCIL;
			if (FAILED(device->CreateTexture2D(&td, nullptr, &depthTex)) || FAILED(device->CreateDepthStencilView(depthTex, nullptr, &dsv))) {
				return false;
			}
			depthW = a_w;
			depthH = a_h;
			return true;
		}

		void Set4(float a_out[4], float a_x, float a_y, float a_z, float a_w)
		{
			a_out[0] = a_x, a_out[1] = a_y, a_out[2] = a_z, a_out[3] = a_w;
		}

		void SetColor(float a_out[4], const RE::Color& a_c)
		{
			Set4(a_out, a_c.red / 255.0f, a_c.green / 255.0f, a_c.blue / 255.0f, 0.0f);
		}

		template <class F>
		void InteriorValue(RE::TESObjectCELL* a_cell, RE::INTERIOR_DATA::Inherit a_flag, F a_get)
		{
			auto*      own = a_cell->GetLighting();
			auto*      tmpl = a_cell->GetRuntimeData().lightingTemplate;
			const bool inherit = tmpl && own->lightingTemplateInheritanceFlags.any(a_flag);
			a_get(inherit ? tmpl->data : *own, inherit ? tmpl : nullptr);
		}

		// Skyrim's lighting at the camera, in camera space.
		void GatherLighting(FrameConstants& a_fc, const RE::NiPoint3& a_cam, const RE::NiPoint3& a_right, const RE::NiPoint3& a_up, const RE::NiPoint3& a_back)
		{
			auto toCam = [&](const RE::NiPoint3& a_w) { return RE::NiPoint3{ a_w.Dot(a_right), a_w.Dot(a_up), a_w.Dot(a_back) }; };
			// Plain daylight, should any of Skyrim's lighting be missing.
			RE::NiPoint3 sun{ 0.3f, -0.4f, 0.87f };
			Set4(a_fc.sunColor, 0.9f, 0.85f, 0.75f, 0.0f);
			for (auto& a : a_fc.ambient) {
				Set4(a, 0.45f, 0.47f, 0.5f, 0.0f);
			}

			auto*      player = RE::PlayerCharacter::GetSingleton();
			auto*      cell = player ? player->GetParentCell() : nullptr;
			const bool interior = cell && cell->IsInteriorCell() && cell->GetLighting();
			auto*      sky = RE::Sky::GetSingleton();
			auto*      ssn = RE::BSShaderManager::State::GetSingleton().shadowSceneNode[0];

			RE::NiDirectionalLight* dirLight = nullptr;
			if (ssn) {
				auto* bsSun = ssn->GetRuntimeData().sunLight;
				if (bsSun && bsSun->light) {
					dirLight = netimmerse_cast<RE::NiDirectionalLight*>(bsSun->light.get());
				}
			}
			if (dirLight) {
				const auto  dir = dirLight->GetWorldDirection();
				const float len = dir.Length();
				if (len > 1e-4f) {
					sun = { -dir.x / len, -dir.y / len, -dir.z / len };
				}
				const auto& ld = dirLight->GetLightRuntimeData();
				const float fade = ld.fade > 0.0f && ld.fade < 16.0f ? ld.fade : 1.0f;
				Set4(a_fc.sunColor, ld.diffuse.red * fade, ld.diffuse.green * fade, ld.diffuse.blue * fade, 0.0f);
			}
			const auto s = toCam(sun);
			Set4(a_fc.sunDir, s.x, s.y, s.z, 0.0f);

			using Inherit = RE::INTERIOR_DATA::Inherit;
			if (interior) {
				InteriorValue(cell, Inherit::kAmbientColor, [&](const RE::INTERIOR_DATA& a_d, RE::BGSLightingTemplate* a_t) {
					const auto& dal = a_t ? a_t->directionalAmbientLightingColors.directional : a_d.directionalAmbientLightingColors.directional;
					SetColor(a_fc.ambient[0], dal.x.min);
					SetColor(a_fc.ambient[1], dal.x.max);
					SetColor(a_fc.ambient[2], dal.y.min);
					SetColor(a_fc.ambient[3], dal.y.max);
					SetColor(a_fc.ambient[4], dal.z.min);
					SetColor(a_fc.ambient[5], dal.z.max);
				});
			} else if (sky) {
				for (int axis = 0; axis < 3; ++axis) {
					const auto& plus = sky->directionalAmbientColors[axis][0];
					const auto& minus = sky->directionalAmbientColors[axis][1];
					Set4(a_fc.ambient[axis * 2], minus.red, minus.green, minus.blue, 0.0f);
					Set4(a_fc.ambient[axis * 2 + 1], plus.red, plus.green, plus.blue, 0.0f);
				}
			}

			// The point lights nearest to reaching the camera.
			struct Candidate
			{
				float        reach;
				RE::NiPoint3 pos;
				float        radius;
				RE::NiColor  color;
			};
			std::vector<Candidate> candidates;
			auto consider = [&](RE::BSLight* a_light) {
				if (!a_light || !a_light->pointLight || !a_light->light) {
					return;
				}
				auto*       light = a_light->light.get();
				const auto& ld = light->GetLightRuntimeData();
				const float radius = ld.radius.x;
				if (!(radius > 1.0f) || light->GetFlags().any(RE::NiAVObject::Flag::kHidden)) {
					return;
				}
				const float dimmer = std::clamp(a_light->lodDimmer, 0.0f, 1.0f) * ld.fade;
				const auto  pos = light->world.translate;
				const float reach = pos.GetDistance(a_cam) - radius;
				if (reach > 200.0f || std::fabs(dimmer) < 1e-3f) {
					return;
				}
				candidates.push_back({ reach, pos, radius, { ld.diffuse.red * dimmer, ld.diffuse.green * dimmer, ld.diffuse.blue * dimmer } });
			};
			if (ssn) {
				auto& rd = ssn->GetRuntimeData();
				for (auto& light : rd.activeLights) {
					consider(light.get());
				}
				for (auto& light : rd.activeShadowLights) {
					consider(light.get());
				}
			}
			std::ranges::sort(candidates, {}, &Candidate::reach);
			const auto count = std::min<std::size_t>(candidates.size(), 8);
			for (std::size_t k = 0; k < count; ++k) {
				const auto& c = candidates[k];
				const auto  p = toCam(c.pos - a_cam);
				Set4(a_fc.lightPos[k], p.x, p.y, p.z, 1.0f / c.radius);
				Set4(a_fc.lightColor[k], c.color.red, c.color.green, c.color.blue, 0.0f);
			}
			a_fc.sunColor[3] = static_cast<float>(count);
			Set4(a_fc.camRight, a_right.x, a_right.y, a_right.z, 0.0f);
			Set4(a_fc.camUp, a_up.x, a_up.y, a_up.z, 0.0f);
			Set4(a_fc.camBack, a_back.x, a_back.y, a_back.z, 0.0f);
		}

		DXGI_FORMAT DepthReadFormat(DXGI_FORMAT a_typeless)
		{
			switch (a_typeless) {
			case DXGI_FORMAT_R16_TYPELESS:
			case DXGI_FORMAT_D16_UNORM:
				return DXGI_FORMAT_R16_UNORM;
			case DXGI_FORMAT_R24G8_TYPELESS:
			case DXGI_FORMAT_D24_UNORM_S8_UINT:
				return DXGI_FORMAT_R24_UNORM_X8_TYPELESS;
			case DXGI_FORMAT_R32_TYPELESS:
			case DXGI_FORMAT_D32_FLOAT:
				return DXGI_FORMAT_R32_FLOAT;
			case DXGI_FORMAT_R32G8X24_TYPELESS:
				return DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS;
			default:
				return DXGI_FORMAT_UNKNOWN;
			}
		}

		// Skyrim's sun shadow cascades for this frame (outside, sunlit): fills the constants and
		// returns the shadow map array, or null. Each cascade's lightTransform maps a world
		// position straight to shadow map uv and depth; it's re-based on the camera here.
		// (SkyCraft's SetSkyrimShadows.)
		ID3D11ShaderResourceView* SetShadows(FrameConstants& a_fc, const RE::NiPoint3& a_cam, const RE::NiPoint3& a_sunDir)
		{
			a_fc.skyShadowSplits[3] = 0.0f;
			if (!sunShadowCopySrv || cascadeCount == 0 || std::chrono::steady_clock::now() - shadowCaptureTime > std::chrono::seconds(1)) {
				return nullptr;
			}
			ID3D11Resource* res = nullptr;
			sunShadowCopySrv->GetResource(&res);
			D3D11_TEXTURE2D_DESC td{};
			static_cast<ID3D11Texture2D*>(res)->GetDesc(&td);
			res->Release();
			bool standard = true;
			for (std::uint32_t k = 0; k < cascadeCount; ++k) {
				const auto& m = cascades[k].m;
				// Row-vector convention (v * M, translation in the last row) unless the matrix says otherwise.
				const bool rowVector = std::fabs(m[0][3]) + std::fabs(m[1][3]) + std::fabs(m[2][3]) < 1e-6f;
				auto       at = [&](int a_r, int a_c) { return double(rowVector ? m[a_r][a_c] : m[a_c][a_r]); };
				for (int out = 0; out < 4; ++out) {
					for (int in = 0; in < 3; ++in) {
						a_fc.skyShadowProj[k][out][in] = float(at(in, out));
					}
					a_fc.skyShadowProj[k][out][3] = float(at(3, out) + at(0, out) * a_cam.x + at(1, out) * a_cam.y + at(2, out) * a_cam.z);
				}
				if (k == 0) {
					// Depth grows along the sunlight (standard) or against it (reversed).
					const double g = at(0, 2) * -a_sunDir.x + at(1, 2) * -a_sunDir.y + at(2, 2) * -a_sunDir.z;
					standard = g > 0.0;
				}
			}
			a_fc.skyShadowSplits[0] = cascades[0].split;
			a_fc.skyShadowSplits[1] = cascadeCount > 1 ? cascades[1].split : cascades[0].split;
			a_fc.skyShadowSplits[3] = static_cast<float>(cascadeCount);
			a_fc.skyShadowParams[0] = 0.0f;
			a_fc.skyShadowParams[1] = cascadeCount > 1 ? 1.0f : 0.0f;
			a_fc.skyShadowParams[2] = 1.0f / static_cast<float>(std::max<UINT>(td.Width, 1));
			a_fc.skyShadowParams[3] = standard ? 1.0f : -1.0f;
			static bool logged = false;
			if (!logged) {
				logged = true;
				logger::info("viewmodel: Skyrim's sun shadows on Faith: {} cascades, maps {}x{}, splits {:.0f} {:.0f}, depth {}", cascadeCount, td.Width, td.Height,
					a_fc.skyShadowSplits[0], a_fc.skyShadowSplits[1], standard ? "standard" : "reversed");
			}
			return sunShadowCopySrv;
		}

		// Everything we touch, saved and put back so Skyrim's renderer finds what it left.
		struct StateBackup
		{
			ID3D11RenderTargetView*   rtv[D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT]{};
			ID3D11DepthStencilView*   dsv{};
			ID3D11BlendState*         blend{};
			float                     factor[4]{};
			UINT                      mask{};
			ID3D11RasterizerState*    raster{};
			ID3D11DepthStencilState*  depth{};
			UINT                      stencil{};
			D3D11_VIEWPORT            vps[D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE]{};
			UINT                      vpCount{ D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE };
			D3D11_PRIMITIVE_TOPOLOGY  topo{};
			ID3D11InputLayout*        layout{};
			ID3D11Buffer*             vb{};
			UINT                      stride{}, offset{};
			ID3D11Buffer*             ib{};
			DXGI_FORMAT               ibFormat{};
			UINT                      ibOffset{};
			ID3D11VertexShader*       vs{};
			ID3D11PixelShader*        ps{};
			ID3D11Buffer*             vsCb[2]{};
			ID3D11Buffer*             psCb[2]{};
			static constexpr UINT     kSlots = D3D11_COMMONSHADER_INPUT_RESOURCE_SLOT_COUNT;
			ID3D11ShaderResourceView* vsAll[kSlots]{};
			ID3D11ShaderResourceView* psAll[kSlots]{};
			ID3D11ShaderResourceView* csAll[kSlots]{};
			ID3D11SamplerState*       samplers[2]{};
			ID3D11GeometryShader*     gs{};
			ID3D11HullShader*         hs{};
			ID3D11DomainShader*       ds{};

			void Save(ID3D11DeviceContext* a_c)
			{
				a_c->OMGetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, rtv, &dsv);
				a_c->OMGetBlendState(&blend, factor, &mask);
				a_c->RSGetState(&raster);
				a_c->OMGetDepthStencilState(&depth, &stencil);
				a_c->RSGetViewports(&vpCount, vps);
				a_c->IAGetPrimitiveTopology(&topo);
				a_c->IAGetInputLayout(&layout);
				a_c->IAGetVertexBuffers(0, 1, &vb, &stride, &offset);
				a_c->IAGetIndexBuffer(&ib, &ibFormat, &ibOffset);
				a_c->VSGetShader(&vs, nullptr, nullptr);
				a_c->PSGetShader(&ps, nullptr, nullptr);
				a_c->VSGetConstantBuffers(0, 2, vsCb);
				a_c->PSGetConstantBuffers(0, 2, psCb);
				a_c->VSGetShaderResources(0, kSlots, vsAll);
				a_c->PSGetShaderResources(0, kSlots, psAll);
				a_c->CSGetShaderResources(0, kSlots, csAll);
				a_c->PSGetSamplers(0, 2, samplers);
				// Whatever extra stages Skyrim (or a mod) left bound would run on our draws too.
				a_c->GSGetShader(&gs, nullptr, nullptr);
				a_c->HSGetShader(&hs, nullptr, nullptr);
				a_c->DSGetShader(&ds, nullptr, nullptr);
				a_c->GSSetShader(nullptr, nullptr, 0);
				a_c->HSSetShader(nullptr, nullptr, 0);
				a_c->DSSetShader(nullptr, nullptr, 0);
			}

			void Restore(ID3D11DeviceContext* a_c)
			{
				a_c->OMSetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, rtv, dsv);
				a_c->OMSetBlendState(blend, factor, mask);
				a_c->RSSetState(raster);
				a_c->OMSetDepthStencilState(depth, stencil);
				a_c->RSSetViewports(vpCount, vps);
				a_c->IASetPrimitiveTopology(topo);
				a_c->IASetInputLayout(layout);
				a_c->IASetVertexBuffers(0, 1, &vb, &stride, &offset);
				a_c->IASetIndexBuffer(ib, ibFormat, ibOffset);
				a_c->VSSetShader(vs, nullptr, 0);
				a_c->PSSetShader(ps, nullptr, 0);
				a_c->VSSetConstantBuffers(0, 2, vsCb);
				a_c->PSSetConstantBuffers(0, 2, psCb);
				a_c->VSSetShaderResources(0, kSlots, vsAll);
				a_c->PSSetShaderResources(0, kSlots, psAll);
				a_c->CSSetShaderResources(0, kSlots, csAll);
				a_c->PSSetSamplers(0, 2, samplers);
				a_c->GSSetShader(gs, nullptr, 0);
				a_c->HSSetShader(hs, nullptr, 0);
				a_c->DSSetShader(ds, nullptr, 0);
				auto drop = [](auto*& a_p) {
					if (a_p) {
						a_p->Release();
						a_p = nullptr;
					}
				};
				for (auto*& r : rtv) {
					drop(r);
				}
				drop(dsv);
				drop(blend);
				drop(raster);
				drop(depth);
				drop(layout);
				drop(vb);
				drop(ib);
				drop(vs);
				drop(ps);
				drop(gs);
				drop(hs);
				drop(ds);
				for (auto*& b : vsCb) {
					drop(b);
				}
				for (auto*& b : psCb) {
					drop(b);
				}
				for (auto*& s : vsAll) {
					drop(s);
				}
				for (auto*& s : psAll) {
					drop(s);
				}
				for (auto*& s : csAll) {
					drop(s);
				}
				drop(samplers[0]);
				drop(samplers[1]);
			}
		};
	}

	// Right after Skyrim renders the sun's shadow maps: copy its cascades (SkyCraft's
	// CaptureSunShadows).
	void CaptureSunShadows(RE::BSShadowLight* a_sun)
	{
		auto* renderer = RE::BSGraphics::Renderer::GetSingleton();
		if (!ready || !device || !renderer || !a_sun) {
			return;
		}
		auto&      descs = a_sun->GetRuntimeData().shadowmapDescriptors;
		const auto count = std::min<std::uint32_t>(descs.size(), 2);
		if (count == 0) {
			return;
		}
		auto*                   context = reinterpret_cast<ID3D11DeviceContext*>(renderer->GetRuntimeData().context);
		ID3D11Resource*         src = nullptr;
		ID3D11DepthStencilView* bound = nullptr;
		context->OMGetRenderTargets(0, nullptr, &bound);
		if (bound) {
			bound->GetResource(&src);
			bound->Release();
		}
		if (!src) {
			auto* tex = reinterpret_cast<ID3D11Texture2D*>(renderer->GetDepthStencilData().depthStencils[RE::RENDER_TARGETS_DEPTHSTENCIL::kSHADOWMAPS].texture);
			if (tex) {
				tex->AddRef();
				src = tex;
			}
		}
		if (!src) {
			return;
		}
		D3D11_TEXTURE2D_DESC sd{};
		static_cast<ID3D11Texture2D*>(src)->GetDesc(&sd);
		if (sunShadowCopy) {
			D3D11_TEXTURE2D_DESC cd{};
			sunShadowCopy->GetDesc(&cd);
			if (cd.Width != sd.Width || cd.Height != sd.Height || cd.Format != sd.Format) {
				Release(sunShadowCopySrv);
				Release(sunShadowCopy);
			}
		}
		if (!sunShadowCopy) {
			D3D11_TEXTURE2D_DESC cd = sd;
			cd.ArraySize = 2;
			cd.MipLevels = 1;
			cd.BindFlags = D3D11_BIND_SHADER_RESOURCE;
			cd.Usage = D3D11_USAGE_DEFAULT;
			cd.CPUAccessFlags = 0;
			cd.MiscFlags = 0;
			D3D11_SHADER_RESOURCE_VIEW_DESC vd{};
			vd.Format = DepthReadFormat(sd.Format);
			vd.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2DARRAY;
			vd.Texture2DArray.MipLevels = 1;
			vd.Texture2DArray.ArraySize = 2;
			const bool ok = vd.Format != DXGI_FORMAT_UNKNOWN && sd.SampleDesc.Count == 1 && SUCCEEDED(device->CreateTexture2D(&cd, nullptr, &sunShadowCopy)) &&
			                SUCCEEDED(device->CreateShaderResourceView(sunShadowCopy, &vd, &sunShadowCopySrv));
			logger::info("viewmodel: capturing the sun's shadow cascades from a {}x{} x{} format {} depth array ({})", sd.Width, sd.Height, sd.ArraySize,
				static_cast<int>(sd.Format), ok ? "ok" : "can't read that format");
			if (!ok) {
				Release(sunShadowCopySrv);
				Release(sunShadowCopy);
				src->Release();
				return;
			}
		}
		const auto& dsl = static_cast<RE::BSShadowDirectionalLight*>(a_sun)->GetShadowDirectionalLightRuntimeData();
		for (std::uint32_t k = 0; k < count; ++k) {
			const UINT slice = descs[k].shadowmapIndex;
			if (slice < sd.ArraySize) {
				context->CopySubresourceRegion(sunShadowCopy, D3D11CalcSubresource(0, k, 1), 0, 0, 0, src, D3D11CalcSubresource(0, slice, sd.MipLevels), nullptr);
			}
			std::memcpy(cascades[k].m, &descs[k].lightTransform.m, sizeof(cascades[k].m));
			cascades[k].split = k < 3 ? dsl.endSplitDistances[k] : 0.0f;
			cascades[k].slice = slice;
		}
		cascadeCount = count;
		src->Release();
		shadowCaptureTime = std::chrono::steady_clock::now();
	}

	// Where Faith goes: Skyrim's HDR scene (before its tone mapping), or, drawn late (just before
	// the HUD, once Community Shaders and Skyrim's post-processing are done), whatever the HUD is
	// about to draw into.
	struct Target
	{
		ID3D11RenderTargetView* rtv = nullptr;
		ID3D11RenderTargetView* motion = nullptr;
		ID3D11Texture2D*        tex = nullptr;
		D3D11_TEXTURE2D_DESC    desc{};
	};

	bool PickTarget(bool a_late, Target& a_out)
	{
		auto* renderer = RE::BSGraphics::Renderer::GetSingleton();
		if (!renderer) {
			return false;
		}
		auto& rd = renderer->GetRuntimeData();
		auto* context = reinterpret_cast<ID3D11DeviceContext*>(rd.context);
		if (a_late) {
			ID3D11RenderTargetView* bound = nullptr;
			context->OMGetRenderTargets(1, &bound, nullptr);
			if (!bound) {
				bound = reinterpret_cast<ID3D11RenderTargetView*>(rd.renderTargets[RE::RENDER_TARGETS::kFRAMEBUFFER].RTV);
				if (bound) {
					bound->AddRef();
				}
			}
			if (!bound) {
				return false;
			}
			ID3D11Resource* res = nullptr;
			bound->GetResource(&res);
			a_out.rtv = bound;
			bound->Release();  // Skyrim keeps its own reference
			if (!res) {
				return false;
			}
			a_out.tex = static_cast<ID3D11Texture2D*>(res);
			res->Release();
		} else {
			a_out.rtv = reinterpret_cast<ID3D11RenderTargetView*>(rd.renderTargets[RE::RENDER_TARGETS::kMAIN].RTV);
			a_out.tex = reinterpret_cast<ID3D11Texture2D*>(rd.renderTargets[RE::RENDER_TARGETS::kMAIN].texture);
			a_out.motion = reinterpret_cast<ID3D11RenderTargetView*>(rd.renderTargets[RE::RENDER_TARGETS::kMOTION_VECTOR].RTV);
		}
		if (!a_out.rtv || !a_out.tex) {
			return false;
		}
		a_out.tex->GetDesc(&a_out.desc);
		static bool logged[2]{};
		if (!logged[a_late]) {
			logged[a_late] = true;
			logger::info("viewmodel: drawing {}: target {}x{} format {}", a_late ? "late, before the HUD" : "into the world", a_out.desc.Width, a_out.desc.Height,
				static_cast<int>(a_out.desc.Format));
		}
		return true;
	}

	D3D11_VIEWPORT SceneViewport(UINT a_width, UINT a_height)
	{
		float rx = 1.0f, ry = 1.0f;
		if (auto* st = RE::BSGraphics::State::GetSingleton()) {
			rx = st->GetRuntimeData().dynamicResolutionWidthRatio;
			ry = st->GetRuntimeData().dynamicResolutionHeightRatio;
		}
		if (!(rx > 0.1f && rx <= 1.0f) || !(ry > 0.1f && ry <= 1.0f)) {
			rx = ry = 1.0f;
		}
		static bool logged = false;
		if (!logged) {
			logged = true;
			if (auto* renderer = RE::BSGraphics::Renderer::GetSingleton()) {
				auto* ctx = reinterpret_cast<ID3D11DeviceContext*>(renderer->GetRuntimeData().context);
				D3D11_VIEWPORT bound{};
				UINT           n = 1;
				ctx->RSGetViewports(&n, &bound);
				ID3D11RenderTargetView* rtv = nullptr;
				ctx->OMGetRenderTargets(1, &rtv, nullptr);
				int which = rtv ? -2 : -1;
				auto& rd = renderer->GetRuntimeData();
				for (int i = 0; rtv && i < RE::RENDER_TARGETS::kTOTAL; ++i) {
					if (reinterpret_cast<ID3D11RenderTargetView*>(rd.renderTargets[i].RTV) == rtv) {
						which = i;
					}
				}
				if (rtv) {
					rtv->Release();
				}
				logger::info("viewmodel: scene {}x{}, dynamic resolution {:.3f} x {:.3f}; after the world Skyrim has viewport {:.0f}x{:.0f} at ({:.0f}, {:.0f}) and render target {} (main is {})",
					a_width, a_height, rx, ry, bound.Width, bound.Height, bound.TopLeftX, bound.TopLeftY, which, static_cast<int>(RE::RENDER_TARGETS::kMAIN));
			}
		}
		return D3D11_VIEWPORT{ 0.0f, 0.0f, std::floor(a_width * rx + 0.5f), std::floor(a_height * ry + 0.5f), 0.0f, 1.0f };
	}

	// Mirror's Edge's speed blur over the scene (before Skyrim's tone mapping), at strength
	// a_amount (MotionPacked.r).
	void SpeedBlur(float a_amount, bool a_late)
	{
		if (!ready || blurFailed || std::fabs(a_amount) < 0.002f) {
			return;
		}
		auto* renderer = RE::BSGraphics::Renderer::GetSingleton();
		if (!renderer) {
			return;
		}
		auto& rd = renderer->GetRuntimeData();
		auto* context = reinterpret_cast<ID3D11DeviceContext*>(rd.context);
		Target target;
		if (!context || !PickTarget(a_late, target)) {
			return;
		}
		auto*      sceneTex = target.tex;
		auto*      rtv = target.rtv;
		const auto sd = target.desc;
		if (a_late) {
			auto* fb = reinterpret_cast<ID3D11Texture2D*>(rd.renderTargets[RE::RENDER_TARGETS::kFRAMEBUFFER].texture);
			D3D11_TEXTURE2D_DESC fd{};
			if (fb) {
				fb->GetDesc(&fd);
			}
			if (!fb || fd.Width != sd.Width || fd.Height != sd.Height) {
				return;  // whatever's bound isn't the frame: blurring it could paint over the screen
			}
		}
		if (!blurVs) {
			ID3DBlob *vsb = nullptr, *psb = nullptr, *errors = nullptr;
			auto compile = [&](const char* a_entry, const char* a_target, ID3DBlob** a_out) {
				const auto hr = D3DCompile(kBlurShader, sizeof(kBlurShader) - 1, "faith_speedblur", nullptr, nullptr, a_entry, a_target, D3DCOMPILE_OPTIMIZATION_LEVEL3, 0, a_out, &errors);
				if (FAILED(hr)) {
					logger::error("speed blur shader {}: {}", a_entry, errors ? static_cast<const char*>(errors->GetBufferPointer()) : "?");
				}
				Release(errors);
				return SUCCEEDED(hr);
			};
			if (!compile("VS", "vs_5_0", &vsb) || !compile("PS", "ps_5_0", &psb)) {
				Release(vsb);
				blurFailed = true;
				return;
			}
			device->CreateVertexShader(vsb->GetBufferPointer(), vsb->GetBufferSize(), nullptr, &blurVs);
			device->CreatePixelShader(psb->GetBufferPointer(), psb->GetBufferSize(), nullptr, &blurPs);
			Release(vsb);
			Release(psb);
			D3D11_BUFFER_DESC cb{};
			cb.Usage = D3D11_USAGE_DYNAMIC;
			cb.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
			cb.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;
			cb.ByteWidth = sizeof(BlurConstants);
			device->CreateBuffer(&cb, nullptr, &blurCb);
			D3D11_SAMPLER_DESC smp{};
			smp.Filter = D3D11_FILTER_MIN_MAG_MIP_LINEAR;
			smp.AddressU = smp.AddressV = smp.AddressW = D3D11_TEXTURE_ADDRESS_CLAMP;
			smp.MaxLOD = D3D11_FLOAT32_MAX;
			device->CreateSamplerState(&smp, &blurSampler);
			D3D11_BLEND_DESC bld{};
			for (auto& t : bld.RenderTarget) {
				t.RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_RED | D3D11_COLOR_WRITE_ENABLE_GREEN | D3D11_COLOR_WRITE_ENABLE_BLUE;
			}
			device->CreateBlendState(&bld, &blurBlend);
			if (!blurVs || !blurPs || !blurCb || !blurSampler || !blurBlend) {
				blurFailed = true;
				return;
			}
			logger::info("speed blur: Mirror's Edge's TdMotionBlurShader ready");
		}
		if (sceneCopy) {
			D3D11_TEXTURE2D_DESC cd{};
			sceneCopy->GetDesc(&cd);
			if (cd.Width != sd.Width || cd.Height != sd.Height || cd.Format != sd.Format) {
				Release(sceneCopySrv);
				Release(sceneCopy);
			}
		}
		if (!sceneCopy) {
			D3D11_TEXTURE2D_DESC cd = sd;
			cd.MipLevels = 1;
			cd.ArraySize = 1;
			cd.BindFlags = D3D11_BIND_SHADER_RESOURCE;
			cd.Usage = D3D11_USAGE_DEFAULT;
			cd.CPUAccessFlags = 0;
			cd.MiscFlags = 0;
			D3D11_SHADER_RESOURCE_VIEW_DESC vd{};
			vd.Format = sd.Format;
			if (vd.Format == DXGI_FORMAT_R16G16B16A16_TYPELESS) {
				vd.Format = DXGI_FORMAT_R16G16B16A16_FLOAT;
			} else if (vd.Format == DXGI_FORMAT_R8G8B8A8_TYPELESS) {
				vd.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
			} else if (vd.Format == DXGI_FORMAT_B8G8R8A8_TYPELESS) {
				vd.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
			} else if (vd.Format == DXGI_FORMAT_R10G10B10A2_TYPELESS) {
				vd.Format = DXGI_FORMAT_R10G10B10A2_UNORM;
			}
			vd.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
			vd.Texture2D.MipLevels = 1;
			if (sd.SampleDesc.Count != 1 || FAILED(device->CreateTexture2D(&cd, nullptr, &sceneCopy)) || FAILED(device->CreateShaderResourceView(sceneCopy, &vd, &sceneCopySrv))) {
				logger::warn("speed blur: can't copy the scene ({}x{} format {}); off", sd.Width, sd.Height, static_cast<int>(sd.Format));
				Release(sceneCopySrv);
				Release(sceneCopy);
				blurFailed = true;
				return;
			}
		}
		const auto sv = a_late ? D3D11_VIEWPORT{ 0.0f, 0.0f, static_cast<float>(sd.Width), static_cast<float>(sd.Height), 0.0f, 1.0f } : SceneViewport(sd.Width, sd.Height);
		StateBackup backup;
		backup.Save(context);
		context->CopyResource(sceneCopy, sceneTex);
		BlurConstants bc{};
		const float  w = static_cast<float>(sd.Width), hgt = static_cast<float>(sd.Height);
		bc.rect[0] = sv.TopLeftX / w;
		bc.rect[1] = sv.TopLeftY / hgt;
		bc.rect[2] = sv.Width / w;
		bc.rect[3] = sv.Height / hgt;
		bc.clampUv[0] = (sv.TopLeftX + 0.5f) / w;
		bc.clampUv[1] = (sv.TopLeftY + 0.5f) / hgt;
		bc.clampUv[2] = (sv.TopLeftX + sv.Width - 0.5f) / w;
		bc.clampUv[3] = (sv.TopLeftY + sv.Height - 0.5f) / hgt;
		bc.motion[0] = a_amount;
		D3D11_MAPPED_SUBRESOURCE m{};
		if (SUCCEEDED(context->Map(blurCb, 0, D3D11_MAP_WRITE_DISCARD, 0, &m))) {
			std::memcpy(m.pData, &bc, sizeof(bc));
			context->Unmap(blurCb, 0);
		}
		context->OMSetRenderTargets(1, &rtv, nullptr);
		context->RSSetViewports(1, &sv);
		context->RSSetState(raster);
		const float factor[4]{};
		context->OMSetBlendState(blurBlend, factor, 0xFFFFFFFF);
		context->OMSetDepthStencilState(nullptr, 0);
		context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
		context->IASetInputLayout(nullptr);
		context->VSSetShader(blurVs, nullptr, 0);
		context->PSSetShader(blurPs, nullptr, 0);
		context->VSSetConstantBuffers(0, 1, &blurCb);
		context->PSSetConstantBuffers(0, 1, &blurCb);
		context->PSSetShaderResources(0, 1, &sceneCopySrv);
		context->PSSetSamplers(0, 1, &blurSampler);
		context->Draw(3, 0);
		ID3D11ShaderResourceView* none = nullptr;
		context->PSSetShaderResources(0, 1, &none);
		backup.Restore(context);
	}

	float ArmsFov(const FaithFrame& a_frame, float a_worldFovDeg, float a_aspect)
	{
		const float worldV = 2.0f * std::atan(std::tan(a_worldFovDeg * 0.5f * 0.0174532925f) * 0.75f);  // Skyrim's FOV: horizontal at 4:3
		const float worldH = 2.0f * std::atan(std::tan(worldV * 0.5f) * a_aspect);
		float       t = std::clamp((-a_frame.pitch - 0.35f) / 0.55f, 0.0f, 1.0f);
		t = t * t * (3.0f - 2.0f * t);
		return 100.0f * 0.0174532925f * (1.0f - t) + worldH * t;
	}

	float SkyrimFovFor(const FaithFrame& a_frame, float a_worldFovDeg)
	{
		float aspect = 16.0f / 9.0f;
		if (auto* renderer = RE::BSGraphics::Renderer::GetSingleton()) {
			if (auto* tex = reinterpret_cast<ID3D11Texture2D*>(renderer->GetRuntimeData().renderTargets[RE::RENDER_TARGETS::kMAIN].texture)) {
				D3D11_TEXTURE2D_DESC d{};
				tex->GetDesc(&d);
				aspect = static_cast<float>(d.Width) / static_cast<float>(std::max(d.Height, 1u));
			}
		}
		const float h = ArmsFov(a_frame, a_worldFovDeg, aspect);
		const float v = 2.0f * std::atan(std::tan(h * 0.5f) / aspect);
		return 2.0f * std::atan(std::tan(v * 0.5f) * (4.0f / 3.0f)) * 57.2957795f;
	}

	float BodyScreenScale(const FaithFrame& a_frame, float a_worldFovDeg)
	{
		float aspect = 16.0f / 9.0f;
		if (auto* renderer = RE::BSGraphics::Renderer::GetSingleton()) {
			if (auto* tex = reinterpret_cast<ID3D11Texture2D*>(renderer->GetRuntimeData().renderTargets[RE::RENDER_TARGETS::kMAIN].texture)) {
				D3D11_TEXTURE2D_DESC d{};
				tex->GetDesc(&d);
				aspect = static_cast<float>(d.Width) / static_cast<float>(std::max(d.Height, 1u));
			}
		}
		// Skyrim's FOV setting is horizontal at 4:3.
		const float worldV = 2.0f * std::atan(std::tan(a_worldFovDeg * 0.5f * 0.0174532925f) * 0.75f);
		const float worldH = 2.0f * std::atan(std::tan(worldV * 0.5f) * aspect);
		const float arms = ArmsFov(a_frame, a_worldFovDeg, aspect);
		return std::tan(worldH * 0.5f) / std::tan(arms * 0.5f);
	}

	void SetVisible(bool a_on) { visible = a_on; }
	bool Visible() { return visible; }
	bool Ready() { return ready; }

	namespace
	{
		bool EnsureReady(::Faith* a_faith)
		{
			if (ready || failed) {
				return ready;
			}
			auto* renderer = RE::BSGraphics::Renderer::GetSingleton();
			if (!renderer) {
				return false;
			}
			auto& rd = renderer->GetRuntimeData();
			auto* dev = reinterpret_cast<ID3D11Device*>(rd.forwarder);
			if (!rd.context || !dev || !Init(a_faith, dev)) {
				failed = true;
				logger::error("viewmodel: couldn't set up drawing Faith's body; Skyrim's first-person arms stay");
				return false;
			}
			ready = true;
			logger::info("viewmodel: drawing Faith's own body");
			return true;
		}

		// The app's grid (128 px a metre square: dark edges, a lighter cross), on a colour.
		ID3D11ShaderResourceView* GridTexture(float a_r, float a_g, float a_b, bool a_grid)
		{
			constexpr std::uint32_t   N = 128;
			std::vector<std::uint8_t> px(N * N * 4);
			for (std::uint32_t y = 0; y < N; ++y) {
				for (std::uint32_t x = 0; x < N; ++x) {
					const auto  edge = std::min({ x, y, N - 1 - x, N - 1 - y });
					const bool  half = x == N / 2 || y == N / 2;
					const float v = !a_grid ? 1.0f : (edge == 0 ? 150.0f : edge == 1 ? 185.0f : half ? 222.0f : 245.0f) / 255.0f;
					auto*       p = &px[(y * N + x) * 4];
					p[0] = static_cast<std::uint8_t>(std::clamp(v * a_r, 0.0f, 1.0f) * 255.0f + 0.5f);
					p[1] = static_cast<std::uint8_t>(std::clamp(v * a_g, 0.0f, 1.0f) * 255.0f + 0.5f);
					p[2] = static_cast<std::uint8_t>(std::clamp(v * a_b, 0.0f, 1.0f) * 255.0f + 0.5f);
					p[3] = 255;
				}
			}
			return MakeTexture(px.data(), N, N);
		}
	}

	void DrawCourse(::Faith* a_faith, bool a_late)
	{
		if (!a_faith || !EnsureReady(a_faith)) {
			return;
		}
		auto* renderer = RE::BSGraphics::Renderer::GetSingleton();
		auto* worldCam = RE::Main::WorldRootCamera();
		if (!renderer || !worldCam) {
			return;
		}
		auto&      rd = renderer->GetRuntimeData();
		auto*      context = reinterpret_cast<ID3D11DeviceContext*>(rd.context);
		const auto n = faith_course_mesh(a_faith, nullptr, 0);
		if (n == 0) {
			return;
		}
		if (!courseLooks[0]) {
			// The app's materials: roof, wall, runner, prop, finish, skyline (plain), metal (plain).
			courseLooks[0] = GridTexture(0.80f, 0.81f, 0.83f, true);
			courseLooks[1] = GridTexture(0.95f, 0.95f, 0.96f, true);
			courseLooks[2] = GridTexture(0.86f, 0.10f, 0.07f, true);
			courseLooks[3] = GridTexture(0.62f, 0.70f, 0.78f, true);
			courseLooks[4] = GridTexture(1.0f, 0.55f, 0.1f, true);
			courseLooks[5] = GridTexture(0.97f, 0.98f, 1.0f, false);
			courseLooks[6] = GridTexture(0.18f, 0.19f, 0.21f, false);
		}
		Target target;
		if (!PickTarget(a_late, target)) {
			return;
		}
		const auto           sd = target.desc;
		const auto&          depth = renderer->GetDepthStencilData().depthStencils[RE::RENDER_TARGETS_DEPTHSTENCIL::kMAIN];
		auto*                worldDsv = reinterpret_cast<ID3D11DepthStencilView*>(depth.views[0]);
		D3D11_TEXTURE2D_DESC dd{};
		if (depth.texture) {
			reinterpret_cast<ID3D11Texture2D*>(depth.texture)->GetDesc(&dd);
		}
		if (!worldDsv || dd.Width != sd.Width || dd.Height != sd.Height) {
			static bool warned = false;
			if (!warned) {
				warned = true;
				logger::warn("course: Skyrim's depth ({}x{}) doesn't match the target ({}x{}): not drawn", dd.Width, dd.Height, sd.Width, sd.Height);
			}
			return;
		}

		// Skyrim's camera (NiCamera: column 0 looks, 1 is up, 2 right), whatever view it's in.
		const auto&        R = worldCam->world.rotate;
		const RE::NiPoint3 cam = worldCam->world.translate;
		const RE::NiPoint3 right{ R.entry[0][2], R.entry[1][2], R.entry[2][2] };
		const RE::NiPoint3 up{ R.entry[0][1], R.entry[1][1], R.entry[2][1] };
		const RE::NiPoint3 back{ -R.entry[0][0], -R.entry[1][0], -R.entry[2][0] };
		FrameConstants     fc{};
		GatherLighting(fc, cam, right, up, back);
		const auto& w2c = worldCam->GetRuntimeData().worldToCam;
		for (int r = 0; r < 4; ++r) {
			for (int c = 0; c < 3; ++c) {
				fc.worldViewProj[r][c] = w2c[r][c];
			}
			fc.worldViewProj[r][3] = float(double(w2c[r][3]) + double(w2c[r][0]) * cam.x + double(w2c[r][1]) * cam.y + double(w2c[r][2]) * cam.z);
		}
		// Last frame's camera, taking this frame's camera-relative positions (the course's motion
		// for TAA). The first frame: none.
		static float lastW2c[4][4]{};
		static bool  haveLast = false;
		for (int r = 0; r < 4; ++r) {
			for (int c = 0; c < 4; ++c) {
				fc.prevWorldViewProj[r][c] = fc.worldViewProj[r][c];
			}
			if (haveLast) {
				for (int c = 0; c < 3; ++c) {
					fc.prevWorldViewProj[r][c] = lastW2c[r][c];
				}
				fc.prevWorldViewProj[r][3] = float(double(lastW2c[r][3]) + double(lastW2c[r][0]) * cam.x + double(lastW2c[r][1]) * cam.y + double(lastW2c[r][2]) * cam.z);
			}
		}
		for (int r = 0; r < 4; ++r) {
			for (int c = 0; c < 4; ++c) {
				lastW2c[r][c] = w2c[r][c];
			}
		}
		haveLast = true;
		const auto& fr = worldCam->GetRuntimeData2().viewFrustum;
		auto        depthAt = [&](float a_dist) {
			const RE::NiPoint3 p = back * -a_dist;
			const float        z = fc.worldViewProj[2][0] * p.x + fc.worldViewProj[2][1] * p.y + fc.worldViewProj[2][2] * p.z + fc.worldViewProj[2][3];
			const float        w = fc.worldViewProj[3][0] * p.x + fc.worldViewProj[3][1] * p.y + fc.worldViewProj[3][2] * p.z + fc.worldViewProj[3][3];
			return z / w;
		};
		const bool                reversed = depthAt(std::max(fr.fNear, 1.0f) * 2.0f) > depthAt(std::max(fr.fFar * 0.25f, 100.0f));
		auto*                     player = RE::PlayerCharacter::GetSingleton();
		auto*                     pcell = player ? player->GetParentCell() : nullptr;
		const RE::NiPoint3        sunWorld = right * fc.sunDir[0] + up * fc.sunDir[1] + back * fc.sunDir[2];
		ID3D11ShaderResourceView* shadowSrv = !(pcell && pcell->IsInteriorCell()) && sunWorld.z > 0.03f ? SetShadows(fc, cam, sunWorld) : nullptr;

		// Its triangles in camera space (the shader turns them back to camera-relative world).
		courseSrc.resize(n);
		const auto got = std::min(faith_course_mesh(a_faith, courseSrc.data(), n), n);
		courseVerts.resize(got);
		for (std::uint32_t i = 0; i < got; ++i) {
			const auto&        v = courseSrc[i];
			const RE::NiPoint3 rel{ v.pos[0] - cam.x, v.pos[1] - cam.y, v.pos[2] - cam.z };
			const RE::NiPoint3 nw{ v.normal[0], v.normal[1], v.normal[2] };
			auto&              o = courseVerts[i];
			o = {};
			o.pos[0] = rel.Dot(right), o.pos[1] = rel.Dot(up), o.pos[2] = rel.Dot(back);
			o.normal[0] = nw.Dot(right), o.normal[1] = nw.Dot(up), o.normal[2] = nw.Dot(back);
			o.tangent[0] = 1.0f, o.tangent[3] = 1.0f;
			o.uv[0] = v.uv[0], o.uv[1] = v.uv[1];
		}
		if (!courseVb || courseVbSize < got) {
			Release(courseVb);
			courseVbSize = std::max<UINT>(got, 4096);
			D3D11_BUFFER_DESC bd{};
			bd.Usage = D3D11_USAGE_DYNAMIC;
			bd.BindFlags = D3D11_BIND_VERTEX_BUFFER;
			bd.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;
			bd.ByteWidth = courseVbSize * sizeof(FaithVertex);
			if (FAILED(device->CreateBuffer(&bd, nullptr, &courseVb))) {
				courseVbSize = 0;
				return;
			}
		}

		StateBackup backup;
		backup.Save(context);
		D3D11_MAPPED_SUBRESOURCE m{};
		if (SUCCEEDED(context->Map(frameCb, 0, D3D11_MAP_WRITE_DISCARD, 0, &m))) {
			std::memcpy(m.pData, &fc, sizeof(fc));
			context->Unmap(frameCb, 0);
		}
		if (SUCCEEDED(context->Map(courseVb, 0, D3D11_MAP_WRITE_DISCARD, 0, &m))) {
			std::memcpy(m.pData, courseVerts.data(), courseVerts.size() * sizeof(FaithVertex));
			context->Unmap(courseVb, 0);
		}
		ObjectConstants oc{ { 0.0f, 0.0f, 0.0f, 0.0f }, { 1.0f, a_late ? 1.0f : 0.0f, 0.0f, 1.0f } };
		if (SUCCEEDED(context->Map(objectCb, 0, D3D11_MAP_WRITE_DISCARD, 0, &m))) {
			std::memcpy(m.pData, &oc, sizeof(oc));
			context->Unmap(objectCb, 0);
		}
		const auto sv = a_late ? D3D11_VIEWPORT{ 0.0f, 0.0f, static_cast<float>(sd.Width), static_cast<float>(sd.Height), 0.0f, 1.0f } : SceneViewport(sd.Width, sd.Height);
		ID3D11RenderTargetView* targets[2] = { target.rtv, target.motion };
		context->OMSetRenderTargets(targets[1] ? 2 : 1, targets, worldDsv);
		context->OMSetDepthStencilState(reversed ? depthStateRev : depthState, 0);
		context->RSSetViewports(1, &sv);
		context->RSSetState(raster);
		const float factor[4]{};
		context->OMSetBlendState(blend, factor, 0xFFFFFFFF);
		context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
		context->IASetInputLayout(layout);
		const UINT stride = sizeof(FaithVertex), offset = 0;
		context->IASetVertexBuffers(0, 1, &courseVb, &stride, &offset);
		context->VSSetShader(vs, nullptr, 0);
		context->PSSetShader(ps, nullptr, 0);
		ID3D11Buffer* cbs[2] = { frameCb, objectCb };
		context->VSSetConstantBuffers(0, 2, cbs);
		context->PSSetConstantBuffers(0, 2, cbs);
		ID3D11SamplerState* samplers[2] = { sampler, pointSampler };
		context->PSSetSamplers(0, 2, samplers);
		// One draw a look (the triangles come grouped by it).
		for (std::uint32_t start = 0; start < got;) {
			const auto    look = std::min<std::uint32_t>(courseSrc[start].look, 6);
			std::uint32_t end = start;
			while (end < got && std::min<std::uint32_t>(courseSrc[end].look, 6) == look) {
				++end;
			}
			ID3D11ShaderResourceView* srvs[4] = { courseLooks[look] ? courseLooks[look] : white, white, white, shadowSrv ? shadowSrv : noShadowSrv };
			context->PSSetShaderResources(0, 4, srvs);
			context->Draw(end - start, start);
			start = end;
		}
		backup.Restore(context);
		static bool logged = false;
		if (!logged) {
			logged = true;
			logger::info("course: drawing {} triangles {}", got / 3, a_late ? "late, before the HUD" : "into the world");
		}
	}

	void Draw(::Faith* a_faith, const FaithFrame& a_frame, float a_worldFovDeg, bool a_late)
	{
		if (!visible || failed || !a_faith || !a_frame.animated || !EnsureReady(a_faith)) {
			return;
		}
		auto* renderer = RE::BSGraphics::Renderer::GetSingleton();
		if (!renderer) {
			return;
		}
		auto& rd = renderer->GetRuntimeData();
		auto* context = reinterpret_cast<ID3D11DeviceContext*>(rd.context);
		Target target;
		if (!PickTarget(a_late, target)) {
			return;
		}
		auto*      rtv = target.rtv;
		const auto sd = target.desc;
		if (!EnsureDepth(sd.Width, sd.Height)) {
			return;
		}
		// Upscalers draw the scene into part of the target and scale it up after: drawn into the
		// world, Faith goes in the same part; drawn late, the whole screen.
		const auto sv = a_late ? D3D11_VIEWPORT{ 0.0f, 0.0f, static_cast<float>(sd.Width), static_cast<float>(sd.Height), 0.0f, 1.0f } : SceneViewport(sd.Width, sd.Height);

		// The projection: Mirror's Edge's first-person field of view (Model1pFOV 100,
		// horizontal), blending to the world's as you look down so the legs meet the ground.
		const float aspect = sv.Width / std::max(sv.Height, 1.0f);
		const float h = ArmsFov(a_frame, a_worldFovDeg, aspect);
		const float v = 2.0f * std::atan(std::tan(h * 0.5f) / aspect);
		const float zn = 0.7f, zf = 3000.0f;
		FrameConstants fc{};
		fc.proj[0][0] = 1.0f / std::tan(h * 0.5f);
		fc.proj[1][1] = 1.0f / std::tan(v * 0.5f);
		fc.proj[2][2] = zf / (zn - zf);
		fc.proj[2][3] = zn * zf / (zn - zf);
		fc.proj[3][2] = -1.0f;
		const RE::NiPoint3 cam{ a_frame.cam_pos.x, a_frame.cam_pos.y, a_frame.cam_pos.z };
		const RE::NiPoint3 right{ a_frame.cam_right.x, a_frame.cam_right.y, a_frame.cam_right.z };
		const RE::NiPoint3 up{ a_frame.cam_up.x, a_frame.cam_up.y, a_frame.cam_up.z };
		const RE::NiPoint3 back{ -a_frame.cam_forward.x, -a_frame.cam_forward.y, -a_frame.cam_forward.z };
		GatherLighting(fc, cam, right, up, back);

		// Skyrim's own camera, for the legs: they're in the world (its depth hides them behind
		// walls), as the app draws them.
		auto*       worldCam = RE::Main::WorldRootCamera();
		const auto& depth = renderer->GetDepthStencilData().depthStencils[RE::RENDER_TARGETS_DEPTHSTENCIL::kMAIN];
		auto*       worldDsv = reinterpret_cast<ID3D11DepthStencilView*>(depth.views[0]);
		bool        legsInWorld = false, reversed = false;
		ID3D11ShaderResourceView* heldSrv = nullptr;
		if (worldCam && worldDsv && depth.texture) {
			D3D11_TEXTURE2D_DESC dd{};
			reinterpret_cast<ID3D11Texture2D*>(depth.texture)->GetDesc(&dd);
			const auto& w2c = worldCam->GetRuntimeData().worldToCam;
			const auto  wc = worldCam->world.translate;
			for (int r = 0; r < 4; ++r) {
				for (int c = 0; c < 3; ++c) {
					fc.worldViewProj[r][c] = w2c[r][c];
				}
				fc.worldViewProj[r][3] = float(double(w2c[r][3]) + double(w2c[r][0]) * wc.x + double(w2c[r][1]) * wc.y + double(w2c[r][2]) * wc.z);
			}
			// Depth convention, from the matrix: does depth shrink with distance (reversed)?
			const auto& fr = worldCam->GetRuntimeData2().viewFrustum;
			auto        depthAt = [&](float a_dist) {
                const RE::NiPoint3 p = back * -a_dist;
                const float        z = fc.worldViewProj[2][0] * p.x + fc.worldViewProj[2][1] * p.y + fc.worldViewProj[2][2] * p.z + fc.worldViewProj[2][3];
                const float        w = fc.worldViewProj[3][0] * p.x + fc.worldViewProj[3][1] * p.y + fc.worldViewProj[3][2] * p.z + fc.worldViewProj[3][3];
                return z / w;
			};
			reversed = depthAt(std::max(fr.fNear, 1.0f) * 2.0f) > depthAt(std::max(fr.fFar * 0.25f, 100.0f));
			legsInWorld = dd.Width == sd.Width && dd.Height == sd.Height && (worldCam->world.translate - cam).Length() < 1.0f;
			if (GetConfig().heldInGrip && depth.depthSRV && (worldCam->world.translate - cam).Length() < 1.0f) {
				// Drawn late, this target is the whole screen and Skyrim's scene its scaled part.
				const auto scene = SceneViewport(dd.Width, dd.Height);
				const float sx = a_late ? scene.Width / static_cast<float>(sd.Width) : static_cast<float>(dd.Width) / static_cast<float>(sd.Width);
				const float sy = a_late ? scene.Height / static_cast<float>(sd.Height) : static_cast<float>(dd.Height) / static_cast<float>(sd.Height);
				Set4(fc.heldDepth, std::max(fr.fNear, 0.1f), fr.fFar, reversed ? 1.0f : 0.0f, 1.0f);
				Set4(fc.heldScale, sx, sy, GetConfig().heldRange, 0.0f);
				heldSrv = reinterpret_cast<ID3D11ShaderResourceView*>(depth.depthSRV);
			}
			static bool logged = false;
			if (!logged) {
				logged = true;
				logger::info("viewmodel: Skyrim's depth {}x{} format {}, {} Z; legs {}", dd.Width, dd.Height, static_cast<int>(dd.Format), reversed ? "reversed" : "standard",
					legsInWorld ? "drawn into the world" : "drawn over it");
			}
		}

		// Skyrim's sun shadows (outside, in sunlight).
		auto*              player = RE::PlayerCharacter::GetSingleton();
		auto*              pcell = player ? player->GetParentCell() : nullptr;
		const bool         interior = pcell && pcell->IsInteriorCell();
		const RE::NiPoint3 sunWorld = right * fc.sunDir[0] + up * fc.sunDir[1] + back * fc.sunDir[2];
		ID3D11ShaderResourceView* shadowSrv = !interior && sunWorld.z > 0.03f ? SetShadows(fc, cam, sunWorld) : nullptr;

		StateBackup backup;
		backup.Save(context);
		{
			D3D11_MAPPED_SUBRESOURCE m{};
			if (SUCCEEDED(context->Map(frameCb, 0, D3D11_MAP_WRITE_DISCARD, 0, &m))) {
				std::memcpy(m.pData, &fc, sizeof(fc));
				context->Unmap(frameCb, 0);
			}
		}
		ID3D11RenderTargetView* targets[2] = { rtv, target.motion };
		context->RSSetViewports(1, &sv);
		context->RSSetState(raster);
		const float factor[4]{};
		context->OMSetBlendState(blend, factor, 0xFFFFFFFF);
		context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
		context->IASetInputLayout(layout);
		context->VSSetShader(vs, nullptr, 0);
		context->PSSetShader(ps, nullptr, 0);
		ID3D11Buffer* cbs[2] = { frameCb, objectCb };
		context->VSSetConstantBuffers(0, 2, cbs);
		context->PSSetConstantBuffers(0, 2, cbs);
		ID3D11SamplerState* samplers[2] = { sampler, pointSampler };
		context->PSSetSamplers(0, 2, samplers);
		ID3D11ShaderResourceView* shadowMaps = shadowSrv ? shadowSrv : noShadowSrv;

		auto drawPart = [&](std::uint32_t a_index, bool a_world) {
			auto& part = parts[a_index];
			if (!faith_body_skin(a_faith, a_index, part.verts.data())) {
				return;
			}
			D3D11_MAPPED_SUBRESOURCE m{};
			if (FAILED(context->Map(part.vb, 0, D3D11_MAP_WRITE_DISCARD, 0, &m))) {
				return;
			}
			std::memcpy(m.pData, part.verts.data(), part.verts.size() * sizeof(FaithVertex));
			context->Unmap(part.vb, 0);
			const UINT stride = sizeof(FaithVertex), offset = 0;
			context->IASetVertexBuffers(0, 1, &part.vb, &stride, &offset);
			context->IASetIndexBuffer(part.ib, DXGI_FORMAT_R32_UINT, 0);
			for (const auto& sec : part.sections) {
				const auto*               mat = sec.material < part.materials.size() ? &part.materials[sec.material] : nullptr;
				ID3D11ShaderResourceView* srvs[5] = { mat && mat->colour ? mat->colour : white, mat && mat->normal ? mat->normal : white,
					mat && mat->spec ? mat->spec : white, shadowMaps, a_world ? nullptr : heldSrv };
				context->PSSetShaderResources(0, 5, srvs);
				ObjectConstants oc{ { mat && mat->normal ? 1.0f : 0.0f, mat && mat->spec ? 1.0f : 0.0f, 0.0f, 0.0f }, { a_world ? 1.0f : 0.0f, a_late ? 1.0f : 0.0f, 0.0f, 0.0f } };
				D3D11_MAPPED_SUBRESOURCE om{};
				if (SUCCEEDED(context->Map(objectCb, 0, D3D11_MAP_WRITE_DISCARD, 0, &om))) {
					std::memcpy(om.pData, &oc, sizeof(oc));
					context->Unmap(objectCb, 0);
				}
				context->DrawIndexed(sec.index_count, sec.first_index, 0);
			}
		};

		// The legs first, in the world: Skyrim's camera and depth (walls hide them).
		if (legsInWorld) {
			context->OMSetRenderTargets(targets[1] ? 2 : 1, targets, worldDsv);
			context->OMSetDepthStencilState(reversed ? depthStateRev : depthState, 0);
			for (std::uint32_t p = 0; p < parts.size(); ++p) {
				if (parts[p].info.legs) {
					drawPart(p, true);
				}
			}
		}
		// Then the arms and torso over everything (and the legs too, if they couldn't go in the
		// world): Mirror's Edge's foreground body, its own depth, its own field of view.
		context->OMSetRenderTargets(targets[1] ? 2 : 1, targets, dsv);
		context->ClearDepthStencilView(dsv, D3D11_CLEAR_DEPTH, 1.0f, 0);
		context->OMSetDepthStencilState(depthState, 0);
		for (std::uint32_t p = 0; p < parts.size(); ++p) {
			if (!parts[p].info.legs || !legsInWorld) {
				drawPart(p, false);
			}
		}
		backup.Restore(context);
	}
}
