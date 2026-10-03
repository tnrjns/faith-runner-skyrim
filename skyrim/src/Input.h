#pragma once

namespace faith::Input
{
	void Install();
	// While on, Skyrim's own player controls don't get Faith's keys or the mouse look.
	void SetCapturing(bool a_on);
	// This frame's controls (and clears the per-frame presses and mouse motion).
	FaithInput Take(float a_sensitivity);
	// The toggle key was pressed since the last call.
	bool TakeToggle();
	// The viewmodel key (Faith's body / Skyrim's arms) was pressed since the last call.
	bool TakeViewmodelToggle();
	// The idle key was pressed since the last call.
	bool TakeIdle();
	// The course's respawn key was pressed (only while on a course, when it's kept from Skyrim).
	bool TakeRespawn();
	// The walk key toggled walking since the last call: now walking or not.
	std::optional<bool> TakeWalkChange();
	void SetCourse(bool a_on);
	bool TakeSurvey();
	// Releases everything (focus moved to a menu).
	void Clear();
}
