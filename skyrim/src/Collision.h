#pragma once

namespace faith::Collision
{
	// Skyrim's solid world in the box a_radius across and a_height up and down from a_center
	// (game units), as triangles (9 floats each) into a_out. False if there's no Havok world.
	// a_candidates: the thin capsules and boxes that may be ziplines, swing poles or beams.
	bool Harvest(const RE::NiPoint3& a_center, float a_radius, float a_up, float a_down, std::vector<float>& a_out, std::vector<FaithFixtureCandidate>* a_candidates = nullptr);
	// The same read in the background (it takes tens of milliseconds): false if one is already
	// running. TakeHarvest hands over a finished one (on the main thread).
	bool HarvestAsync(const RE::NiPoint3& a_center, float a_radius, float a_up, float a_down, bool a_candidates);
	bool TakeHarvest(std::vector<float>& a_out, std::vector<FaithFixtureCandidate>& a_candidates);
	// What moves (doors, gates: the bodies the last Harvest found outside the fixed island), where
	// it is now, as triangles. False (a_out untouched) when none has moved since the last call.
	bool CollectMoving(std::vector<float>& a_out);
	// Let go of the bodies held for that (Faith off, a load).
	void Forget();
	// What a ray from a_from to a_to hits, as Mirror's Edge's step sound set (faith_set_surfaces).
	std::uint32_t SurfaceAt(const RE::NiPoint3& a_from, const RE::NiPoint3& a_to);
	// Is there something to stand on under a_at (within a_below units down) in these triangles?
	bool GroundUnder(const std::vector<float>& a_tris, const RE::NiPoint3& a_at, float a_below);
	// Survey: all of the world's loaded collision around the player (a_radius, a_height) written to
	// <SKSE log folder>\FaithSurvey_<worldspace form ID>.bin for the parkour tool. Returns the path.
	std::string Survey(float a_radius, float a_height);
}
