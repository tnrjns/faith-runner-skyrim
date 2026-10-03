#pragma once

namespace faith::Body
{
	// One of the player's skeletons, bound to Faith's.
	struct Skeleton
	{
		std::int32_t                  id = -1;
		bool                          tried = false;
		std::vector<std::string>      bones;
		std::vector<RE::NiAVObject*>  nodes;  // the live nodes, by bone
		std::vector<FaithXform>       out;
		RE::NiAVObject*               root = nullptr;

		bool Bind(Faith* a_faith, RE::PlayerCharacter* a_player, bool a_firstPerson);
		// Pose the live skeleton under a_live (the player's 3D root) with this frame's animation.
		void Apply(Faith* a_faith, RE::NiAVObject* a_live, std::uint32_t a_flags = 0);
		void Reset();
		// Some bone's local transform isn't what Apply last wrote (something else posed it).
		bool Changed() const;
		void Enclose(RE::NiAVObject* a_live);
	};
}
