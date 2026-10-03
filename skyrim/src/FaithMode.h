#pragma once

namespace faith
{
	// After the game's data has loaded: start Faith and hook the frame.
	void Install();
	// A save was loaded or a new game started.
	void OnGameLoaded();

	// For the settings page: Faith's state, and things to do on the next frame (the menu pauses
	// the game, so they happen once it closes).
	bool IsActive();
	int  CurrentView();  // 0 her own body, 1 Skyrim's body, 2 Skyrim's arms
	void RequestToggle();
	void RequestView(int a_view);
	void RequestIdle();
	// The app's training courses: start one (0 Moves, 1 Rooftops, 2 Springboard, 3 Training),
	// leave, or go back to a checkpoint.
	void RequestCourse(int a_map);
	void RequestLeaveCourse();
	void RequestCheckpoint(int a_checkpoint);
	bool OnCourse();
	// Faith herself (for the settings page to read the course's state).
	::Faith* Handle();
	// Saves made on a course remember where it was started from (SKSE co-save).
	void RegisterSaves();
}
