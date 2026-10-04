#include "Menu.h"

#include "Config.h"
#include "FaithMode.h"

#include <filesystem>

#include <SKSEMenuFramework.h>

namespace faith::Menu
{
	namespace
	{
		bool dirty = false;

		// Keys you can bind (DirectInput scan codes). Faith's own movement keys (W A S D, Space,
		// Shift, Ctrl, C, Q, F) are left out.
		struct Key
		{
			const char*   name;
			std::uint32_t code;
		};
		constexpr Key kKeys[] = {
			{ "F1", 0x3B }, { "F2", 0x3C }, { "F3", 0x3D }, { "F4", 0x3E }, { "F5", 0x3F }, { "F6", 0x40 },
			{ "F7", 0x41 }, { "F8", 0x42 }, { "F9", 0x43 }, { "F10", 0x44 }, { "F11", 0x57 }, { "F12", 0x58 },
			{ "1", 0x02 }, { "2", 0x03 }, { "3", 0x04 }, { "4", 0x05 }, { "5", 0x06 }, { "6", 0x07 },
			{ "7", 0x08 }, { "8", 0x09 }, { "9", 0x0A }, { "0", 0x0B },
			{ "E", 0x12 }, { "R", 0x13 }, { "T", 0x14 }, { "Y", 0x15 }, { "U", 0x16 }, { "I", 0x17 },
			{ "O", 0x18 }, { "P", 0x19 }, { "G", 0x22 }, { "H", 0x23 }, { "J", 0x24 }, { "K", 0x25 },
			{ "L", 0x26 }, { "Z", 0x2C }, { "X", 0x2D }, { "V", 0x2F }, { "B", 0x30 }, { "N", 0x31 },
			{ "M", 0x32 }, { "Tab", 0x0F }, { "Caps Lock", 0x3A }, { "Left Alt", 0x38 },
			{ "Insert", 0xD2 }, { "Delete", 0xD3 }, { "Home", 0xC7 }, { "End", 0xCF }, { "Page Up", 0xC9 },
			{ "Page Down", 0xD1 }, { "Numpad 0", 0x52 }, { "Numpad 1", 0x4F }, { "Numpad 2", 0x50 },
			{ "Numpad 3", 0x51 }, { "Numpad 4", 0x4B }, { "Numpad 5", 0x4C }, { "Numpad 6", 0x4D },
			{ "Numpad 7", 0x47 }, { "Numpad 8", 0x48 }, { "Numpad 9", 0x49 },
		};

		void Changed(bool a_changed)
		{
			dirty |= a_changed;
		}

		void Tooltip(const char* a_text)
		{
			if (ImGuiMCP::IsItemHovered()) {
				ImGuiMCP::SetTooltip("%s", a_text);
			}
		}

		void KeyPicker(const char* a_label, std::uint32_t& a_key)
		{
			const char* current = nullptr;
			for (const auto& k : kKeys) {
				if (k.code == a_key) {
					current = k.name;
				}
			}
			const auto preview = current ? std::string(current) : std::format("{:#x}", a_key);
			if (ImGuiMCP::BeginCombo(a_label, preview.c_str())) {
				for (const auto& k : kKeys) {
					const bool selected = k.code == a_key;
					if (ImGuiMCP::Selectable(k.name, selected)) {
						a_key = k.code;
						dirty = true;
					}
				}
				ImGuiMCP::EndCombo();
			}
		}

		void __stdcall RenderGeneral()
		{
			auto& c = EditConfig();
			ImGuiMCP::Text("Faith is %s", IsActive() ? "on" : "off");
			ImGuiMCP::SameLine();
			if (ImGuiMCP::Button(IsActive() ? "Switch her off" : "Switch her on")) {
				RequestToggle();
			}
			Tooltip("Happens when you close the menu.");

			ImGuiMCP::SeparatorText("First person");
			const char* views[] = { "Faith's own body", "Skyrim's whole body" };
			int         view = std::min(CurrentView(), 1);
			if (ImGuiMCP::Combo("View", &view, views, 2)) {
				RequestView(view);
				c.faithViewmodel = view == 0;
				dirty = true;
			}
			Tooltip("Also what you start in. The view key switches between them in game.");
			Changed(ImGuiMCP::Checkbox("Speed blur when running fast", &c.speedBlur));
			Changed(ImGuiMCP::Checkbox("Weapons in her grip", &c.heldInGrip));
			Tooltip("In her own body view, what Skyrim's hands hold shows in front of her arms, so her fingers close round it.");
			Changed(ImGuiMCP::Checkbox("Animate the third-person body too", &c.thirdPersonBody));
			Tooltip("Takes effect the next time Faith is switched on.");

			ImGuiMCP::SeparatorText("The world");
			Changed(ImGuiMCP::Checkbox("Ziplines, swing poles and beams in Skyrim", &c.worldFixtures));
			Changed(ImGuiMCP::Checkbox("Step up onto knee-high steps", &c.autoStepUp));
			Tooltip("Mirror's Edge's own auto step-up (written into the game but switched off there): walking into a step up to about half a metre, "
					"she steps up onto it instead of stopping. Skyrim's stone steps are often taller than she walks up by herself.");
			Tooltip("Skyrim's own cables, high bars and planks over drops work as Mirror's Edge's: found in the collision around you.");

			Changed(ImGuiMCP::Checkbox("Use Skyrim's stamina", &c.stamina));
			Tooltip("Sprinting, wallruns and wallclimbs drain stamina at Skyrim's sprint rate; with none left she can't sprint. Mirror's Edge has no stamina: off is the game's way.");

			ImGuiMCP::SeparatorText("Combat");
			Changed(ImGuiMCP::Checkbox("Attacks land on people", &c.meleeHits));
			Tooltip("Mirror's Edge's own hit tests and damage: a punch lands on someone close in front, kicks when the limb reaches them.");
			Changed(ImGuiMCP::SliderFloat("Damage", &c.meleeDamageMult, 0.25f, 4.0f, "x %.2f"));
			Tooltip("x Mirror's Edge's damage, taken as Skyrim health: punch 33.5, air kick 60-100, slide kick 60, wallrun kick 80.");
			Changed(ImGuiMCP::Checkbox("Takedowns finish them", &c.takedownKills));
			Tooltip("Mirror's Edge's disarm (the takedown key, right stick on a pad): she takes their weapon, and at the end they go down. "
					"Off: they're staggered and fight on. Essential characters only go down to their knees.");

			ImGuiMCP::SeparatorText("Idles");
			if (ImGuiMCP::Button("Play an idle")) {
				RequestIdle();
			}
			Tooltip("Plays when you close the menu, if Faith is standing still. Each press plays the next one.");
			ImGuiMCP::TextDisabled("She also plays them by herself after 30-40 s standing still.");

			ImGuiMCP::SeparatorText("Feel");
			Changed(ImGuiMCP::SliderFloat("Mouse sensitivity", &c.mouseSensitivity, 0.1f, 3.0f, "%.2f"));
			Changed(ImGuiMCP::SliderFloat("Sound volume", &c.soundVolume, 0.0f, 1.0f, "%.2f"));
			Tooltip("Faith's sounds from Mirror's Edge: footsteps, breathing, landings, wind.");
			Changed(ImGuiMCP::Checkbox("Faith on as soon as a save loads", &c.startEnabled));
			Changed(ImGuiMCP::SliderFloat("Walking speed", &c.walkStick, 0.1f, 0.6f, "%.2f of a run"));
			Tooltip("With walking toggled on (Caps Lock): the keys as this much of a stick. Mirror's Edge sprints only with the stick right forward.");

			ImGuiMCP::SeparatorText("Skyrim's whole body");
			Changed(ImGuiMCP::Checkbox("Hands where Faith's appear on screen", &c.bodyScreenMatch));
			Tooltip("Faith's arms are drawn with Mirror's Edge's 100-degree view; Skyrim's body with the world's. On: its hands are put as far across the screen as hers appear (when they're in view), so it looks like her arms. Off: exactly where hers are in the world.");
			Changed(ImGuiMCP::SliderFloat("Camera ahead of the eyes", &c.bodyCameraForward, 0.0f, 15.0f, "%.1f units"));
			Tooltip("Keeps the camera out in front of the neck and collar.");
			Changed(ImGuiMCP::SliderFloat("More looking down", &c.bodyCameraForwardDown, 0.0f, 25.0f, "%.1f units"));
			Tooltip("Keeps it out of the chest when you look down at your feet.");

			ImGuiMCP::SeparatorText("Reset");
			// Two clicks within 3 seconds, so a stray one can't wipe your keys.
			static double armed = -10.0, doneAt = -10.0;
			const double  now = ImGuiMCP::GetTime();
			const bool    confirming = now - armed < 3.0;
			if (ImGuiMCP::Button(confirming ? "Click again to reset everything" : "Reset all settings")) {
				if (confirming) {
					ResetConfig();
					dirty = false;
					armed = -10.0;
					doneAt = now;
				} else {
					armed = now;
				}
			}
			Tooltip("Every setting and key back to how Faith Runner ships, saved to FaithSkyrim.ini. Your Mirror's Edge folder is kept.");
			if (now - doneAt < 3.0) {
				ImGuiMCP::SameLine();
				ImGuiMCP::Text("Settings reset.");
			}
		}

		std::string TimeText(float a_t)
		{
			const int m = static_cast<int>(a_t / 60.0f);
			return std::format("{}:{:05.2f}", m, a_t - m * 60.0f);
		}

		int courseMap = 0;

		void __stdcall RenderCourse()
		{
			ImGuiMCP::TextWrapped("The training maps from the Faith Runner app, built high in the sky above where you are. "
								  "Falling off puts you back at the last checkpoint; the time trial starts as you leave the start.");
			const char* maps[] = { "Moves: springboard, balance beam, swing pole, zipline, door", "Rooftops", "Springboard", "Training: every move in order" };
			ImGuiMCP::Combo("Map", &courseMap, maps, 4);
			if (ImGuiMCP::Button(OnCourse() ? "Start it again" : "Start")) {
				RequestCourse(courseMap);
			}
			Tooltip("Happens when you close the menu. Switches Faith on if she isn't.");
			if (!OnCourse()) {
				return;
			}
			ImGuiMCP::SameLine();
			if (ImGuiMCP::Button("Leave the course")) {
				RequestLeaveCourse();
			}
			Tooltip("Back to where you started it.");

			auto*       f = Handle();
			FaithCourse s{};
			if (!f || !faith_course_status(f, &s)) {
				return;
			}
			ImGuiMCP::SeparatorText(faith_course_name(f));
			ImGuiMCP::Text("Time: %s%s", s.running ? TimeText(s.time).c_str() : "not started", s.running ? "" : " (leave the start area)");
			ImGuiMCP::Text("Last run: %s   Best: %s", s.last >= 0.0f ? TimeText(s.last).c_str() : "-", s.best >= 0.0f ? TimeText(s.best).c_str() : "-");
			ImGuiMCP::SeparatorText("Checkpoints");
			for (std::uint32_t i = 0; i < s.checkpoints; ++i) {
				const auto label = std::format("{}{}##cp{}", faith_course_checkpoint_name(f, i), i == s.checkpoint ? "  (current)" : "", i);
				if (ImGuiMCP::Button(label.c_str())) {
					RequestCheckpoint(static_cast<int>(i));
				}
			}
			ImGuiMCP::TextDisabled("The first one restarts the time trial.");
		}

		// On the course: its time and checkpoint at the top of the screen, as the app shows them.
		void __stdcall RenderHud()
		{
			if (!OnCourse() || SKSEMenuFramework::IsAnyBlockingWindowOpened()) {
				return;
			}
			auto*       f = Handle();
			FaithCourse s{};
			if (!f || !faith_course_status(f, &s)) {
				return;
			}
			std::string line = s.running ? TimeText(s.time) : std::string("0:00.00");
			if (s.best >= 0.0f) {
				line += "   Best " + TimeText(s.best);
			}
			const std::string sub = std::format("{} - {}", faith_course_name(f), faith_course_checkpoint_name(f, s.checkpoint));
			auto*       draw = ImGuiMCP::GetForegroundDrawList();
			auto*       font = ImGuiMCP::GetFont();
			const float big = ImGuiMCP::GetFontSize() * 1.6f, normal = ImGuiMCP::GetFontSize();
			const float w = ImGuiMCP::GetIO()->DisplaySize.x;
			const auto  bigSize = ImGuiMCP::CalcTextSize(line.c_str());
			const auto  smallSize = ImGuiMCP::CalcTextSize(sub.c_str());
			const float bw = bigSize.x * 1.6f;
			ImGuiMCP::ImDrawListManager::AddText(draw, font, big, ImGuiMCP::ImVec2{ (w - bw) * 0.5f + 2.0f, 32.0f }, IM_COL32(0, 0, 0, 160), line.c_str());
			ImGuiMCP::ImDrawListManager::AddText(draw, font, big, ImGuiMCP::ImVec2{ (w - bw) * 0.5f, 30.0f }, IM_COL32(255, 255, 255, 255), line.c_str());
			ImGuiMCP::ImDrawListManager::AddText(draw, font, normal, ImGuiMCP::ImVec2{ (w - smallSize.x) * 0.5f + 1.0f, 31.0f + bigSize.y * 1.6f + 4.0f }, IM_COL32(0, 0, 0, 160), sub.c_str());
			ImGuiMCP::ImDrawListManager::AddText(draw, font, normal, ImGuiMCP::ImVec2{ (w - smallSize.x) * 0.5f, 30.0f + bigSize.y * 1.6f + 4.0f }, IM_COL32(220, 220, 220, 255), sub.c_str());
		}

		void __stdcall RenderKeys()
		{
			auto& c = EditConfig();
			KeyPicker("Switch Faith on and off", c.toggleKey);
			KeyPicker("First-person view", c.viewmodelKey);
			KeyPicker("Play an idle", c.idleKey);
			KeyPicker("Takedown", c.takedownKey);
			KeyPicker("Reaction Time", c.reactionKey);
			Tooltip("Mirror's Edge's disarm on whoever's close in front of her: from behind, or a snatch from the front.");
			KeyPicker("Walk (toggle)", c.walkKey);
			KeyPicker("Back to the checkpoint (on a course)", c.respawnKey);
			KeyPicker("Survey the collision", c.surveyKey);
			Tooltip("Saves the collision loaded around you for the parkour tool.");
			ImGuiMCP::TextDisabled("Movement keys (W A S D, Space, Shift, Ctrl, C, Q, F) are Faith's own.");
		}

		void __stdcall RenderAdvanced()
		{
			auto& c = EditConfig();
			ImGuiMCP::SeparatorText("Drawing");
			const char* stages[] = { "Automatic", "Into the world", "Late, before the HUD" };
			Changed(ImGuiMCP::Combo("Faith's body drawn", &c.drawStage, stages, 3));
			Tooltip("Automatic: late with Community Shaders, into the world without.\n"
					"Into the world: Skyrim's lighting, legs hidden behind walls.\n"
					"Late: always visible, but over everything.");
			Changed(ImGuiMCP::SliderFloat("Near clip", &c.nearDistance, 0.0f, 15.0f, "%.1f units"));
			Tooltip("Skyrim's near clip while Faith is on (Skyrim's own is 15). Lower: walls close to the camera aren't cut away. 0 leaves Skyrim's.");
			Changed(ImGuiMCP::SliderFloat("Near clip, whole-body view", &c.nearDistanceBody, 0.0f, 15.0f, "%.1f units"));

			ImGuiMCP::SeparatorText("Collision");
			Changed(ImGuiMCP::SliderFloat("Radius", &c.collisionRadius, 1000.0f, 6000.0f, "%.0f units"));
			Tooltip("How much of Skyrim's collision around you Faith moves on (70 units a metre).");
			Changed(ImGuiMCP::SliderFloat("Height", &c.collisionHeight, 500.0f, 4000.0f, "%.0f units"));
			Changed(ImGuiMCP::SliderFloat("Refresh", &c.collisionRefresh, 0.1f, 2.0f, "every %.1f s"));

			ImGuiMCP::SeparatorText("Mirror's Edge");
			ImGuiMCP::TextWrapped("Folder: %s", c.mirrorsEdgeDir.empty() ? "the usual install folders" : c.mirrorsEdgeDir.c_str());
			ImGuiMCP::TextDisabled("Set in FaithSkyrim.ini (read when the game starts).");
		}

		void __stdcall OnEvent(SKSEMenuFramework::Model::EventType a_type)
		{
			if (a_type == SKSEMenuFramework::Model::kCloseMenu && dirty) {
				dirty = false;
				SaveConfig();
			}
		}
	}

	void Register()
	{
		if (!SKSEMenuFramework::IsInstalled()) {
			logger::info("SKSE Menu Framework isn't installed: settings are in FaithSkyrim.ini only");
			return;
		}
		SKSEMenuFramework::SetSection("Faith Runner");
		SKSEMenuFramework::AddSectionItem("General", RenderGeneral);
		SKSEMenuFramework::AddSectionItem("Course", RenderCourse);
		SKSEMenuFramework::AddSectionItem("Keys", RenderKeys);
		SKSEMenuFramework::AddHudElement(RenderHud);
		SKSEMenuFramework::AddSectionItem("Advanced", RenderAdvanced);
		SKSEMenuFramework::AddEvent(OnEvent, 0.0f);
		logger::info("settings page added to SKSE Menu Framework");
	}
}
