#include "Input.h"

#include "Config.h"

namespace faith::Input
{
	namespace
	{
		// DirectInput scan codes.
		constexpr std::uint32_t kW = 0x11, kA = 0x1E, kS = 0x1F, kD = 0x20;
		constexpr std::uint32_t kSpace = 0x39, kLShift = 0x2A, kC = 0x2E, kLCtrl = 0x1D;
		constexpr std::uint32_t kQ = 0x10, kF = 0x21;
		constexpr std::uint32_t kMouseLeft = 0;

		// Faith's keys: Skyrim's player controls don't see these while Faith drives.
		bool IsFaithKey(std::uint32_t a_code)
		{
			switch (a_code) {
			case kW:
			case kA:
			case kS:
			case kD:
			case kSpace:
			case kLShift:
			case kC:
			case kLCtrl:
			case kQ:
			case kF:
				return true;
			default:
				return false;
			}
		}

		std::array<bool, 256> down{};
		bool                  mouseLeft = false;
		// Pressed since the last Take.
		bool jumpPressed = false, crouchPressed = false, turnPressed = false, meleePressed = false;
		bool togglePressed = false, viewmodelPressed = false, idlePressed = false, surveyPressed = false, respawnPressed = false;
		// Skyrim's always-run toggle, for Faith: walking (the keys as half a stick, as on a pad).
		bool walking = false, walkChanged = false;
		std::atomic<bool> onCourse{ false };
		float lookDx = 0.0f, lookDy = 0.0f;
		std::atomic<bool> capturing{ false };

		bool MenuOpen()
		{
			auto* ui = RE::UI::GetSingleton();
			if (!ui) {
				return false;
			}
			for (const auto& menu : ui->menuStack) {
				if (menu && menu->menuFlags.any(RE::UI_MENU_FLAGS::kPausesGame, RE::UI_MENU_FLAGS::kUsesCursor)) {
					return true;
				}
			}
			return false;
		}

		class InputSink final : public RE::BSTEventSink<RE::InputEvent*>
		{
		public:
			static InputSink* Get()
			{
				static InputSink sink;
				return &sink;
			}

			RE::BSEventNotifyControl ProcessEvent(RE::InputEvent* const* a_event, RE::BSTEventSource<RE::InputEvent*>*) override
			{
				if (!a_event) {
					return RE::BSEventNotifyControl::kContinue;
				}
				const bool menu = MenuOpen();
				if (menu) {
					Clear();
				}
				for (auto* e = *a_event; e; e = e->next) {
					switch (e->GetEventType()) {
					case RE::INPUT_EVENT_TYPE::kMouseMove:
						if (!menu) {
							auto* mm = e->AsMouseMoveEvent();
							lookDx += static_cast<float>(mm->mouseInputX);
							lookDy += static_cast<float>(mm->mouseInputY);
						}
						break;
					case RE::INPUT_EVENT_TYPE::kButton:
						{
							auto*      button = e->AsButtonEvent();
							const bool isDown = button->IsDown();
							const bool isUp = button->IsUp();
							if (!isDown && !isUp) {
								break;  // held
							}
							const auto code = button->GetIDCode();
							if (button->GetDevice() == RE::INPUT_DEVICE::kKeyboard && code < 256) {
								if (isDown && code == GetConfig().toggleKey && !menu) {
									togglePressed = true;
								}
								if (isDown && code == GetConfig().viewmodelKey) {
									logger::info("view key pressed{}", menu ? " (a menu is open: ignored)" : "");
									viewmodelPressed = !menu;
								}
								if (isDown && code == GetConfig().idleKey && !menu) {
									idlePressed = true;
								}
								if (isDown && code == GetConfig().surveyKey && !menu) {
									surveyPressed = true;
								}
								if (isDown && code == GetConfig().respawnKey && !menu && onCourse) {
									respawnPressed = true;
								}
								if (isDown && code == GetConfig().walkKey && !menu) {
									walking = !walking;
									walkChanged = true;
								}
								if (menu) {
									break;
								}
								down[code] = isDown;
								if (isDown) {
									jumpPressed |= code == kSpace;
									crouchPressed |= code == kLShift || code == kC || code == kLCtrl;
									turnPressed |= code == kQ;
									meleePressed |= code == kF;
								}
							} else if (button->GetDevice() == RE::INPUT_DEVICE::kMouse && code == kMouseLeft && !menu) {
								mouseLeft = isDown;
								meleePressed |= isDown;
							}
							break;
						}
					default:
						break;
					}
				}
				return RE::BSEventNotifyControl::kContinue;
			}
		};

		bool Swallow(RE::InputEvent* a_event)
		{
			switch (a_event->GetEventType()) {
			case RE::INPUT_EVENT_TYPE::kMouseMove:
			case RE::INPUT_EVENT_TYPE::kThumbstick:
				return true;
			case RE::INPUT_EVENT_TYPE::kButton:
				{
					auto* button = a_event->AsButtonEvent();
					const auto code = button->GetIDCode();
					if (button->GetDevice() == RE::INPUT_DEVICE::kKeyboard) {
						return IsFaithKey(code) || code == GetConfig().toggleKey || code == GetConfig().viewmodelKey || code == GetConfig().idleKey ||
						       code == GetConfig().walkKey ||
						       (onCourse && code == GetConfig().respawnKey);
					}
					if (button->GetDevice() == RE::INPUT_DEVICE::kMouse) {
						return code == kMouseLeft;
					}
					return false;
				}
			default:
				return false;
			}
		}

		// PlayerControls turns input into the Skyrim player's own actions (moving, looking,
		// jumping, sneaking, attacking). While Faith drives, it doesn't get Faith's keys or the
		// mouse look; everything else (activate, weapons, favourites, shouts) still reaches it.
		struct PlayerControlsHook
		{
			static RE::BSEventNotifyControl thunk(RE::PlayerControls* a_this, RE::InputEvent* const* a_event, RE::BSTEventSource<RE::InputEvent*>* a_source)
			{
				if (!capturing || !a_event || !*a_event || MenuOpen()) {
					return func(a_this, a_event, a_source);
				}
				std::vector<RE::InputEvent*> all, keep;
				for (auto* e = *a_event; e; e = e->next) {
					all.push_back(e);
					if (!Swallow(e)) {
						keep.push_back(e);
					}
				}
				if (keep.empty()) {
					return RE::BSEventNotifyControl::kContinue;
				}
				std::vector<RE::InputEvent*> savedNext;
				for (auto* e : all) {
					savedNext.push_back(e->next);
				}
				for (std::size_t i = 0; i < keep.size(); ++i) {
					keep[i]->next = i + 1 < keep.size() ? keep[i + 1] : nullptr;
				}
				RE::InputEvent* head = keep.front();
				const auto      result = func(a_this, &head, a_source);
				for (std::size_t i = 0; i < all.size(); ++i) {
					all[i]->next = savedNext[i];
				}
				return result;
			}
			static inline REL::Relocation<decltype(thunk)> func;
		};
	}

	void Install()
	{
		if (auto* devices = RE::BSInputDeviceManager::GetSingleton()) {
			devices->AddEventSink(InputSink::Get());
		}
		REL::Relocation<std::uintptr_t> vtbl{ RE::VTABLE_PlayerControls[0] };
		PlayerControlsHook::func = vtbl.write_vfunc(0x1, PlayerControlsHook::thunk);
		logger::info("input hooks installed");
	}

	void SetCapturing(bool a_on)
	{
		capturing = a_on;
		if (!a_on) {
			Clear();
		}
	}

	FaithInput Take(float a_sensitivity)
	{
		FaithInput in{};
		in.move_x = (down[kD] ? 1.0f : 0.0f) - (down[kA] ? 1.0f : 0.0f);
		in.move_y = (down[kW] ? 1.0f : 0.0f) - (down[kS] ? 1.0f : 0.0f);
		if (walking) {
			// Mirror's Edge walks with the stick part way: MinBase + (MaxBase - MinBase) x the stick
			// (about 1.3 m/s here), never sprinting.
			in.move_x *= GetConfig().walkStick;
			in.move_y *= GetConfig().walkStick;
		}
		// The app's mouse: 0.0022 radians a count (times the sensitivity setting).
		const float k = 0.0022f * a_sensitivity;
		in.look_right = lookDx * k;
		in.look_up = -lookDy * k;
		in.jump_pressed = jumpPressed;
		in.jump_held = down[kSpace];
		in.crouch_pressed = crouchPressed;
		in.crouch_held = down[kLShift] || down[kC] || down[kLCtrl];
		in.turn_pressed = turnPressed;
		in.melee_pressed = meleePressed;
		jumpPressed = crouchPressed = turnPressed = meleePressed = false;
		lookDx = lookDy = 0.0f;
		return in;
	}

	bool TakeToggle()
	{
		const bool t = togglePressed;
		togglePressed = false;
		return t;
	}

	bool TakeViewmodelToggle()
	{
		const bool t = viewmodelPressed;
		viewmodelPressed = false;
		return t;
	}

	bool TakeSurvey()
	{
		const bool t = surveyPressed;
		surveyPressed = false;
		return t;
	}

	std::optional<bool> TakeWalkChange()
	{
		if (!walkChanged) {
			return std::nullopt;
		}
		walkChanged = false;
		return walking;
	}

	bool TakeRespawn()
	{
		const bool t = respawnPressed;
		respawnPressed = false;
		return t;
	}

	void SetCourse(bool a_on) { onCourse = a_on; }

	bool TakeIdle()
	{
		const bool t = idlePressed;
		idlePressed = false;
		return t;
	}

	void Clear()
	{
		down.fill(false);
		mouseLeft = false;
		jumpPressed = crouchPressed = turnPressed = meleePressed = false;
		lookDx = lookDy = 0.0f;
	}
}
