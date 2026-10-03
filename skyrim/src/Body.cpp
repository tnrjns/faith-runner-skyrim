// Faith's animation on the player's skeletons: the third-person body and the first-person arms.
//
// Each skeleton is bound once (faith_bind_skeleton) from its rest pose, read from the skeleton
// NIF the game uses, and then every frame faith_pose_skeleton gives local transforms for all
// its bones, which are written straight onto the live nodes.
#include "Body.h"

namespace faith::Body
{
	namespace
	{
		FaithXform ToXform(const RE::NiTransform& a_t)
		{
			const auto& m = a_t.rotate.entry;
			// Rotation matrix (column vectors) -> quaternion.
			float       q[4];
			const float tr = m[0][0] + m[1][1] + m[2][2];
			if (tr > 0.0f) {
				const float s = std::sqrt(tr + 1.0f) * 2.0f;
				q[3] = 0.25f * s;
				q[0] = (m[2][1] - m[1][2]) / s;
				q[1] = (m[0][2] - m[2][0]) / s;
				q[2] = (m[1][0] - m[0][1]) / s;
			} else if (m[0][0] > m[1][1] && m[0][0] > m[2][2]) {
				const float s = std::sqrt(1.0f + m[0][0] - m[1][1] - m[2][2]) * 2.0f;
				q[3] = (m[2][1] - m[1][2]) / s;
				q[0] = 0.25f * s;
				q[1] = (m[0][1] + m[1][0]) / s;
				q[2] = (m[0][2] + m[2][0]) / s;
			} else if (m[1][1] > m[2][2]) {
				const float s = std::sqrt(1.0f + m[1][1] - m[0][0] - m[2][2]) * 2.0f;
				q[3] = (m[0][2] - m[2][0]) / s;
				q[0] = (m[0][1] + m[1][0]) / s;
				q[1] = 0.25f * s;
				q[2] = (m[1][2] + m[2][1]) / s;
			} else {
				const float s = std::sqrt(1.0f + m[2][2] - m[0][0] - m[1][1]) * 2.0f;
				q[3] = (m[1][0] - m[0][1]) / s;
				q[0] = (m[0][2] + m[2][0]) / s;
				q[1] = (m[1][2] + m[2][1]) / s;
				q[2] = 0.25f * s;
			}
			FaithXform x{};
			std::memcpy(x.rot, q, sizeof(q));
			x.pos[0] = a_t.translate.x;
			x.pos[1] = a_t.translate.y;
			x.pos[2] = a_t.translate.z;
			x.scale = a_t.scale;
			return x;
		}

		void FromXform(const FaithXform& a_x, RE::NiTransform& a_out)
		{
			const float x = a_x.rot[0], y = a_x.rot[1], z = a_x.rot[2], w = a_x.rot[3];
			auto&       m = a_out.rotate.entry;
			m[0][0] = 1 - 2 * (y * y + z * z);
			m[0][1] = 2 * (x * y - z * w);
			m[0][2] = 2 * (x * z + y * w);
			m[1][0] = 2 * (x * y + z * w);
			m[1][1] = 1 - 2 * (x * x + z * z);
			m[1][2] = 2 * (y * z - x * w);
			m[2][0] = 2 * (x * z - y * w);
			m[2][1] = 2 * (y * z + x * w);
			m[2][2] = 1 - 2 * (x * x + y * y);
			a_out.translate = { a_x.pos[0], a_x.pos[1], a_x.pos[2] };
			a_out.scale = a_x.scale;
		}

		// Every node under a_root (not a_root itself), parents first.
		void Collect(RE::NiAVObject* a_node, std::int32_t a_parent, std::vector<std::string>& a_names, std::vector<std::int32_t>& a_parents,
			std::vector<FaithXform>& a_rest)
		{
			auto* node = a_node ? a_node->AsNode() : nullptr;
			if (!node) {
				return;
			}
			for (auto& child : node->GetChildren()) {
				auto* c = child ? child->AsNode() : nullptr;
				if (!c || c->name.empty()) {
					continue;
				}
				const auto index = static_cast<std::int32_t>(a_names.size());
				a_names.emplace_back(c->name.c_str());
				a_parents.push_back(a_parent);
				a_rest.push_back(ToXform(c->local));
				Collect(c, index, a_names, a_parents, a_rest);
			}
		}

		RE::NiPointer<RE::NiNode> LoadNif(const std::string& a_path)
		{
			RE::NiPointer<RE::NiNode>        root;
			RE::BSModelDB::DBTraits::ArgsType args{};
			args.postProcess = false;
			const auto result = RE::BSModelDB::Demand(a_path.c_str(), root, args);
			if (result != RE::BSResource::ErrorCode::kNone || !root) {
				return nullptr;
			}
			return root;
		}

		// The skeleton NIFs to read the rest pose from: the race's own for the body; for the
		// arms the same file name under _1stPerson, then _1stPerson\skeleton.nif.
		std::vector<std::string> RestPaths(RE::PlayerCharacter* a_player, bool a_firstPerson)
		{
			std::vector<std::string> out;
			auto*                    race = a_player->GetRace();
			const auto*              base = a_player->GetActorBase();
			const bool               female = base && base->GetSex() == RE::SEX::kFemale;
			std::string              body;
			if (race) {
				body = race->skeletonModels[female ? RE::SEXES::kFemale : RE::SEXES::kMale].model.c_str();
			}
			if (!a_firstPerson) {
				if (!body.empty()) {
					out.push_back(body);
				}
				return out;
			}
			if (!body.empty()) {
				const auto slash = body.find_last_of("\\/");
				const auto file = slash == std::string::npos ? body : body.substr(slash + 1);
				out.push_back("Actors\\Character\\_1stPerson\\" + file);
			}
			out.push_back("Actors\\Character\\_1stPerson\\skeleton.nif");
			return out;
		}
	}

	bool Skeleton::Bind(Faith* a_faith, RE::PlayerCharacter* a_player, bool a_firstPerson)
	{
		tried = true;
		auto* live = a_player->Get3D(a_firstPerson);
		if (!live) {
			return false;
		}
		std::vector<std::string>  names;
		std::vector<std::int32_t> parents;
		std::vector<FaithXform>   rest;
		std::string               from;
		for (const auto& path : RestPaths(a_player, a_firstPerson)) {
			if (auto nif = LoadNif(path)) {
				Collect(nif.get(), -1, names, parents, rest);
				if (!names.empty()) {
					from = path;
					break;
				}
			}
		}
		if (names.empty()) {
			// No skeleton file: the live skeleton's current pose stands in for the rest pose.
			Collect(live, -1, names, parents, rest);
			from = "the live skeleton";
		}
		std::vector<const char*> cnames;
		for (const auto& n : names) {
			cnames.push_back(n.c_str());
		}
		id = faith_bind_skeleton(a_faith, a_firstPerson ? 1 : 0, static_cast<std::uint32_t>(names.size()), cnames.data(), parents.data(), rest.data());
		if (id < 0) {
			logger::warn("{} skeleton: couldn't bind ({}): {}", a_firstPerson ? "first-person" : "body", from, faith_last_error());
			return false;
		}
		bones = std::move(names);
		root = nullptr;
		out.assign(bones.size(), {});
		logger::info("{} skeleton: {} bones from {}, {} driven by Faith", a_firstPerson ? "first-person" : "body", bones.size(), from,
			faith_skeleton_mapped(a_faith, id));
		return true;
	}

	void Skeleton::Apply(Faith* a_faith, RE::NiAVObject* a_live, std::uint32_t a_flags)
	{
		if (id < 0 || !a_live) {
			return;
		}
		if (a_live != root) {
			// The 3D was (re)built: find the nodes again.
			root = a_live;
			nodes.assign(bones.size(), nullptr);
			std::size_t found = 0;
			for (std::size_t i = 0; i < bones.size(); ++i) {
				nodes[i] = a_live->GetObjectByName(RE::BSFixedString(bones[i].c_str()));
				found += nodes[i] != nullptr;
			}
			logger::info("skeleton: {} of {} bones found in the live 3D", found, bones.size());
		}
		if (!faith_pose_skeleton_ex(a_faith, id, ToXform(a_live->world), a_flags, out.data())) {
			return;
		}
		for (std::size_t i = 0; i < nodes.size(); ++i) {
			if (nodes[i]) {
				FromXform(out[i], nodes[i]->local);
			}
		}
		RE::NiUpdateData update{};
		a_live->UpdateDownwardPass(update, 0);
		Enclose(a_live);
	}

	// Skyrim culls a mesh whose bounds are off screen, and the bounds come from where its own
	// animation would put the body. Faith's pose puts it elsewhere (the arms where she has them
	// against her camera), so the bounds of everything under the skeleton are set to enclose
	// the posed bones.
	void Skeleton::Enclose(RE::NiAVObject* a_live)
	{
		RE::NiPoint3 centre{};
		int          n = 0;
		for (auto* node : nodes) {
			if (node) {
				centre += node->world.translate;
				++n;
			}
		}
		if (!n) {
			return;
		}
		centre = centre / static_cast<float>(n);
		float radius = 0.0f;
		for (auto* node : nodes) {
			if (node) {
				radius = std::max(radius, node->world.translate.GetDistance(centre));
			}
		}
		radius += 60.0f;  // the skin around the bones
		std::function<void(RE::NiAVObject*)> visit = [&](RE::NiAVObject* a_obj) {
			a_obj->worldBound.center = centre;
			a_obj->worldBound.radius = radius;
			if (auto* node = a_obj->AsNode()) {
				for (auto& child : node->GetChildren()) {
					if (child) {
						visit(child.get());
					}
				}
			}
		};
		visit(a_live);
	}

	bool Skeleton::Changed() const
	{
		RE::NiTransform t;
		for (std::size_t i = 0; i < nodes.size(); ++i) {
			if (!nodes[i]) {
				continue;
			}
			FromXform(out[i], t);
			const auto& l = nodes[i]->local;
			for (int r = 0; r < 3; ++r) {
				for (int c = 0; c < 3; ++c) {
					if (std::fabs(l.rotate.entry[r][c] - t.rotate.entry[r][c]) > 1e-3f) {
						return true;
					}
				}
			}
		}
		return false;
	}

	void Skeleton::Reset()
	{
		id = -1;
		tried = false;
		bones.clear();
		nodes.clear();
		out.clear();
		root = nullptr;
	}

}
