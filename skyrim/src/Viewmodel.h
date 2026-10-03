#pragma once

namespace faith::Viewmodel
{
	// Draw Faith's own first-person body into Skyrim's scene (right after Main::RenderWorld).
	// a_worldFovDeg: Skyrim's world field of view setting.
	// a_late: just before the HUD, after Skyrim's (and Community Shaders') post-processing,
	// instead of into the world before it.
	void Draw(::Faith* a_faith, const FaithFrame& a_frame, float a_worldFovDeg, bool a_late);
	// The training course (faith_course_mesh), with Skyrim's camera into its depth: before Faith's
	// body, so walls hide her legs.
	void DrawCourse(::Faith* a_faith, bool a_late);
	void SetVisible(bool a_on);
	bool Visible();
	// Set up and drawing (false until the first draw, or if setting up failed).
	bool Ready();
	// Right after Skyrim renders the sun's shadow maps (its shadows fall on Faith's body).
	void CaptureSunShadows(RE::BSShadowLight* a_sun);
	// Skyrim's first-person field of view setting that matches Faith's arms this frame (so what
	// Skyrim's hidden first-person hands hold lines up with her hands).
	float SkyrimFovFor(const FaithFrame& a_frame, float a_worldFovDeg);
	// How much narrower the world's view (Skyrim's body is drawn with) is than Faith's arms':
	// tan(world half FOV) / tan(arms' half FOV), for faith_set_screen_scale.
	float BodyScreenScale(const FaithFrame& a_frame, float a_worldFovDeg);
	// Mirror's Edge's speed blur over the scene (after the world, before Skyrim's tone mapping).
	void SpeedBlur(float a_amount, bool a_late);
}
