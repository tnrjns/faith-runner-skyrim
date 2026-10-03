#pragma once

namespace faith
{
	// Data\SKSE\Plugins\FaithSkyrim.ini
	struct Config
	{
		std::string   mirrorsEdgeDir;        // empty: the usual install folders
		std::uint32_t toggleKey = 0x42;      // DirectInput scan code (F8)
		bool          startEnabled = false;
		float         mouseSensitivity = 1.0f;
		float         collisionRadius = 2400.0f;  // units around the player
		float         collisionHeight = 1400.0f;  // units above and below
		float         collisionRefresh = 0.5f;    // seconds
		bool          thirdPersonBody = true;     // animate the third-person body too
		bool          faithViewmodel = true;      // draw Faith's own first-person body
		std::uint32_t viewmodelKey = 0x41;        // switches it and Skyrim's arms (F7)
		float         soundVolume = 0.8f;         // Faith's sounds (0 = off)
		std::uint32_t idleKey = 0x22;             // plays one of Faith's idles (G)
		std::uint32_t walkKey = 0x3A;             // toggles walking, like Skyrim's always-run (Caps Lock)
		float         walkStick = 0.3f;           // walking: the keys as this much of a stick
		float         bodyCameraForward = 5.0f;   // whole-body view: the camera this far ahead of her eye (units)
		float         bodyCameraForwardDown = 10.0f;  // ... and this much more looking straight down
		std::uint32_t respawnKey = 0x13;          // on a training course: back to the checkpoint (R)
		float         courseHeight = 20000.0f;    // how far above you a training course is put (units)
		std::uint32_t surveyKey = 0x44;           // saves the collision around you for the parkour tool (F10)
		float         nearDistance = 3.0f;        // Skyrim's near clip plane while Faith is on (units; 0 = leave it)
		float         nearDistanceBody = 10.0f;   // ... in the whole-body view
		int           drawStage = 0;              // Faith's body: 0 auto, 1 into the world, 2 late (before the HUD)
		bool          speedBlur = true;           // Mirror's Edge's speed blur when running fast
		bool          skyrimBody = true;          // Skyrim's view (F7) shows its whole body, not just the arms
	};

	const Config& GetConfig();
	// For the settings page (SKSE Menu Framework): change it in place, then save it to the ini.
	Config&       EditConfig();
	void          LoadConfig();
	void          SaveConfig();
}
