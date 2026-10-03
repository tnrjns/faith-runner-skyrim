#pragma once

namespace faith::Body
{
	// One of the player's skeletons, bound to Faith's (or someone's, to the takedown's victim).
	struct Skeleton
	{
		std::int32_t                  id = -1;
		bool                          tried = false;
		std::vector<std::string>      bones;
		std::vector<RE::NiAVObject*>  nodes;  // the live nodes, by bone
		std::vector<FaithXform>       out;
		RE::NiAVObject*               root = nullptr;

		bool Bind(Faith* a_faith, RE::Actor* a_actor, bool a_firstPerson);
		// Bound to the takedown's victim side instead (faith_bind_victim), the whole body.
		bool BindVictim(Faith* a_faith, RE::Actor* a_actor);
		// Pose the live skeleton under a_live (the player's 3D root) with this frame's animation.
		void Apply(Faith* a_faith, RE::NiAVObject* a_live, std::uint32_t a_flags = 0);
		// Pose a victim's live skeleton: takedown a_anim, a_time seconds in, standing at a_feet
		// facing a_heading.
		void ApplyVictim(Faith* a_faith, RE::NiAVObject* a_live, std::uint32_t a_anim, float a_time, const RE::NiPoint3& a_feet, float a_heading);
		void Reset();
		// Some bone's local transform isn't what Apply last wrote (something else posed it).
		bool Changed() const;
		void Enclose(RE::NiAVObject* a_live);

	private:
		bool Gather(RE::Actor* a_actor, bool a_firstPerson, std::vector<std::string>& a_names, std::vector<std::int32_t>& a_parents,
			std::vector<FaithXform>& a_rest, std::string& a_from);
		void Find(RE::NiAVObject* a_live);
		void Write(RE::NiAVObject* a_live);
	};
}
