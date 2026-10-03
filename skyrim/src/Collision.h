#pragma once

namespace faith::Collision
{
	// Skyrim's solid world in the box a_radius across and a_height up and down from a_center
	// (game units), as triangles (9 floats each) into a_out. False if there's no Havok world.
	bool Harvest(const RE::NiPoint3& a_center, float a_radius, float a_up, float a_down, std::vector<float>& a_out);
	// Is there something to stand on under a_at (within a_below units down) in these triangles?
	bool GroundUnder(const std::vector<float>& a_tris, const RE::NiPoint3& a_at, float a_below);
	// Survey: all of the world's loaded collision around the player (a_radius, a_height) written to
	// <SKSE log folder>\FaithSurvey_<worldspace form ID>.bin for the parkour tool. Returns the path.
	std::string Survey(float a_radius, float a_height);
}
