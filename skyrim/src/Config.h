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
		float         collisionRefresh = 2.0f;    // seconds (also whenever she's moved a quarter of the radius)
		bool          thirdPersonBody = true;     // animate the third-person body too
		bool          faithViewmodel = true;      // draw Faith's own first-person body
		std::uint32_t viewmodelKey = 0x41;        // switches it and Skyrim's arms (F7)
		float         soundVolume = 0.8f;         // Faith's sounds (0 = off)
		std::uint32_t idleKey = 0x22;             // plays one of Faith's idles (G)
		std::uint32_t walkKey = 0x3A;             // toggles walking, like Skyrim's always-run (Caps Lock)
		float         walkStick = 0.3f;           // walking: the keys as this much of a stick
		float         bodyCameraForward = 5.0f;   // whole-body view: the camera this far ahead of her eye (units)
		float         bodyCameraForwardDown = 10.0f;  // ... and this much more looking straight down
		bool          worldFixtures = true;       // Skyrim's own cables, bars and planks as ziplines, swing poles, beams
		bool          autoStepUp = true;          // steps up onto knee-high steps (Mirror's Edge's own, off in the game)
		bool          bodyScreenMatch = true;     // Skyrim's body's hands where Faith's appear on screen
		bool          heldInGrip = true;          // her fingers close round what Skyrim's hands hold
		float         heldRange = 60.0f;          // ... anything Skyrim drew this close (units) counts as held
		bool          stamina = false;            // sprinting, wallruns and wallclimbs use Skyrim's stamina
		bool          meleeHits = true;           // Faith's attacks land on Skyrim's actors
		float         meleeDamageMult = 1.0f;     // x Mirror's Edge's damage (its hit points as Skyrim's health)
		std::uint32_t takedownKey = 0x2F;         // Mirror's Edge's disarm on whoever's in front of her (V)
		bool          takedownKills = true;       // a takedown finishes them (else they're staggered and fight on)
		std::uint32_t reactionKey = 0x2D;         // Mirror's Edge's Reaction Time (X)
		std::uint32_t respawnKey = 0x13;          // on a training course: back to the checkpoint (R)
		float         courseHeight = 20000.0f;    // how far above you a training course is put (units)
		std::uint32_t surveyKey = 0x44;           // saves the collision around you for the parkour tool (F10)
		float         nearDistance = 3.0f;        // Skyrim's near clip plane while Faith is on (units; 0 = leave it)
		float         nearDistanceBody = 10.0f;   // ... in the whole-body view
		int           drawStage = 0;              // Faith's body: 0 auto, 1 into the world, 2 late (before the HUD)
		bool          speedBlur = true;           // Mirror's Edge's speed blur when running fast
	};

	const Config& GetConfig();
	// For the settings page (SKSE Menu Framework): change it in place, then save it to the ini.
	Config&       EditConfig();
	void          LoadConfig();
	void          SaveConfig();
	// Every setting and key back to how Faith Runner ships, saved to the ini. The Mirror's Edge
	// folder is kept (it's where the game is, not a preference).
	void          ResetConfig();
}
