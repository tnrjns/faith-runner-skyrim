// Faith mode: Mirror's Edge's movement (faith_move) drives Skyrim's player, on Skyrim's own
// collision, with Faith's animations on the player's skeletons and her camera in first person.
//
// The frame (all on the main thread):
// - PlayerCharacter::Update: read the controls, refresh the collision around the player, step
//   Faith, put the player (and its Havok capsule) where she is.
// - Actor::UpdateAnimation and PlayerCamera::Update: Skyrim animates and places the camera, then
//   Faith's pose goes on the skeletons and, in first person, her camera on Skyrim's.
//
// Hooks follow SkyCraft (MIT License, Copyright (c) 2026 chasmlol), which drives Skyrim's player
// the same way on 1.7.104.
#include "FaithMode.h"

#include "Body.h"
#include "Collision.h"
#include "Config.h"
#include "Input.h"
#include "Viewmodel.h"

namespace faith
{
	namespace
	{
		::Faith*     faith = nullptr;
		bool         active = false;
		FaithFrame   frame{};
		bool         haveFrame = false;
		float        collisionTimer = 0.0f;
		RE::NiPoint3 collisionAt{};
		std::vector<float> tris;
		Body::Skeleton body, arms;
		bool         wantEnable = false;
		// Skyrim's first-person meshes we hid while Faith's own body is drawn.
		std::vector<RE::NiPointer<RE::BSGeometry>> hiddenFirstPerson;
		// What first person shows: Faith's own body, Skyrim's whole body (chest and legs too),
		// or Skyrim's first-person arms. F7 goes round them.
		enum class View
		{
			kFaith,
			kSkyrimBody,
			kSkyrimArms,
		};
		View view = View::kFaith;
		// From the settings page, done on the next frame.
		std::atomic<bool> menuToggle{ false }, menuIdle{ false };
		std::atomic<int>  menuView{ -1 };
		// Course requests: kNoRequest, kLeave, or a map to start; a checkpoint to go back to.
		constexpr int     kNoRequest = -2, kLeave = -1;
		std::atomic<int>  menuCourse{ kNoRequest }, menuRespawn{ kNoRequest };

		// On one of the app's training courses: where you were before (put back there after, and
		// on loading a save made on the course).
		bool              onCourse = false;
		RE::NiPoint3      returnPos;
		float             returnHeading = 0.0f;
		std::uint32_t     courseSeq = 0;
		std::optional<std::pair<RE::NiPoint3, float>> loadedReturn;
		// Skyrim's third-person body, shown in first person (kSkyrimBody): what we unhid for the
		// frame's drawing, and its head (the camera is in it) hidden meanwhile.
		std::vector<RE::NiPointer<RE::NiAVObject>> shownPieces;
		std::vector<RE::NiPointer<RE::NiAVObject>> hiddenHead;
		// Where Faith last stood (the safety net if she ever drops through the world).
		RE::NiPoint3 lastGround{};
		bool         haveGround = false;
		// Drawn late (just before the HUD) rather than into the world: Community Shaders lights the
		// world deferred and paints over anything drawn into it.
		bool DrawLate()
		{
			static const bool cs = GetModuleHandleA("CommunityShaders.dll") != nullptr;
			const int         stage = GetConfig().drawStage;
			return stage == 2 || (stage == 0 && cs);
		}

		// The camera is Faith's: first person, or Skyrim's third-person camera that the
		// whole-body view puts at her eyes.
		bool FaithCamera(RE::PlayerCamera* a_camera)
		{
			return a_camera && (a_camera->IsInFirstPerson() || (view == View::kSkyrimBody && a_camera->IsInThirdPerson()));
		}

		bool forcedThirdPerson = false;

		// Skyrim's near clip distance, saved while Faith lowers it.
		float savedNear = -1.0f;
		// Diagnostics: frames the pose (or camera) was changed by Skyrim between our last update
		// and drawing.
		int          diagFrames = 0, diagPoseLost = 0, diagCamLost = 0;
		// How far the drawn camera is from Faith's (position, angle), and how much her camera
		// animation turns the view away from the plain look (degrees), worst over the window.
		float        diagCamOff = 0.0f, diagCamAngle = 0.0f, diagAnimTurn = 0.0f;
		float        diagTimer = 5.0f;
		RE::NiMatrix3 lastCamRot{};
		bool         lastCamSet = false;

		// The camera root's axes as Skyrim builds them, found by watching it follow the look
		// angles we give the player (SkyCraft's method): which of +/- forward, up, right each of
		// its matrix columns is.
		std::array<std::array<int, 6>, 3> axisVotes{};
		int                               axisSamples = 0;
		std::array<int, 3>                axisMap{ 0, 1, 2 };
		bool                              axesKnown = false;
		bool                              axesRejected = false;

		RE::NiPoint3 P(const FaithVec3& a_v) { return { a_v.x, a_v.y, a_v.z }; }
		RE::NiPoint3 Col(const RE::NiMatrix3& a_m, int a_c) { return { a_m.entry[0][a_c], a_m.entry[1][a_c], a_m.entry[2][a_c] }; }

		float AngleDeg(RE::NiPoint3 a_a, RE::NiPoint3 a_b)
		{
			const float la = a_a.Length(), lb = a_b.Length();
			if (la < 1e-6f || lb < 1e-6f) {
				return 180.0f;
			}
			return std::acos(std::clamp(a_a.Dot(a_b) / (la * lb), -1.0f, 1.0f)) * 57.2957795f;
		}

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

		// Skyrim takes the player for its own animations (furniture, horses, kill moves, scenes):
		// Faith lets go meanwhile.
		bool SkyrimBusy(RE::PlayerCharacter* a_player)
		{
			if (a_player->AsActorState()->GetSitSleepState() != RE::SIT_SLEEP_STATE::kNormal || a_player->IsOnMount() || a_player->IsInKillMove()) {
				return true;
			}
			if (auto* camera = RE::PlayerCamera::GetSingleton(); camera && camera->currentState) {
				switch (camera->currentState->id) {
				case RE::CameraState::kFurniture:
				case RE::CameraState::kAnimated:
				case RE::CameraState::kBleedout:
				case RE::CameraState::kDragon:
				case RE::CameraState::kMount:
				case RE::CameraState::kVATS:
					return true;
				default:
					break;
				}
			}
			return false;
		}

		// What the first-person hands hold: anything under the weapon, shield, magic or animation
		// object nodes.
		bool Held(RE::NiAVObject* a_obj, RE::NiAVObject* a_root)
		{
			static const std::array<std::string_view, 8> kHolders{ "WEAPON", "SHIELD", "AnimObjectR", "AnimObjectL", "NPC R MagicNode [RMag]",
				"NPC L MagicNode [LMag]", "MagicEffectsNode", "QUIVER" };
			for (auto* n = a_obj->parent; n && n != a_root; n = n->parent) {
				const std::string_view name = n->name.c_str();
				if (std::ranges::find(kHolders, name) != kHolders.end()) {
					return true;
				}
			}
			return false;
		}

		// Skyrim's first-person arms hidden while Faith's own body is drawn (what they hold stays).
		// Only the meshes: the skeleton stays, posed like Faith's, for weapons and Skyrim's camera.
		void HideFirstPerson(RE::PlayerCharacter* a_player, bool a_hide, bool a_keepHeld = true)
		{
			if (!a_hide) {
				for (auto& mesh : hiddenFirstPerson) {
					if (mesh && mesh->GetAppCulled()) {
						mesh->SetAppCulled(false);
					}
				}
				hiddenFirstPerson.clear();
				return;
			}
			auto* root = a_player ? a_player->Get3D(true) : nullptr;
			if (!root) {
				return;
			}
			RE::BSVisit::TraverseScenegraphGeometries(root, [&](RE::BSGeometry* a_mesh) {
				if (a_keepHeld && Held(a_mesh, root)) {
					return RE::BSVisit::BSVisitControl::kContinue;  // a weapon, shield, spell: it stays, in Faith's grip
				}
				if (!a_mesh->GetAppCulled()) {
					a_mesh->SetAppCulled(true);
					hiddenFirstPerson.emplace_back(a_mesh);
				}
				return RE::BSVisit::BSVisitControl::kContinue;
			});
		}

		bool DrawingFaith(RE::PlayerCharacter* a_player)
		{
			auto* camera = RE::PlayerCamera::GetSingleton();
			return active && haveFrame && frame.animated && view == View::kFaith && camera && camera->IsInFirstPerson() && !SkyrimBusy(a_player);
		}

		// Skyrim's whole body seen in first person (the kSkyrimBody view).
		bool ShowingSkyrimBody(RE::PlayerCharacter* a_player)
		{
			auto* camera = RE::PlayerCamera::GetSingleton();
			return active && haveFrame && frame.animated && view == View::kSkyrimBody && body.id >= 0 && FaithCamera(camera) && !SkyrimBusy(a_player);
		}

		// Skyrim hides the third-person body in first person; for the kSkyrimBody view it's shown
		// for the frame's drawing, without its head (and helmet, hood, hair: the camera is inside
		// it), then put back the way Skyrim had it.
		bool IsHidden(RE::NiAVObject* a_obj)
		{
			return std::ranges::any_of(hiddenHead, [&](const auto& h) { return h.get() == a_obj; });
		}

		// The whole-body view's head (the camera is inside it): the face and hair, anything hung on
		// the head bone (physics hair, helmets), and what's worn on the head. Hidden for the whole
		// frame, from the player's update to the next, so every pass that draws the scene (Community
		// Shaders draws it more than once) leaves it out.
		void HideHead(RE::PlayerCharacter* a_player, bool a_hide)
		{
			for (auto& obj : hiddenHead) {
				obj->SetAppCulled(false);
			}
			hiddenHead.clear();
			auto* root = a_player ? a_player->Get3D(false) : nullptr;
			if (!a_hide || !root) {
				return;
			}
			auto hide = [&](RE::NiAVObject* a_obj) {
				if (a_obj && !a_obj->GetAppCulled()) {
					a_obj->SetAppCulled(true);
					hiddenHead.emplace_back(a_obj);
				}
			};
			// What's worn on the head (helmets, hoods, hair, circlets, ears) is skinned armor, not on
			// the head bone: hidden too, unless it's part of something worn on the body (a hooded robe).
			if (const auto& biped = a_player->GetBiped(false)) {
				using Slot = RE::BIPED_OBJECTS::BIPED_OBJECT;
				for (const auto slot : { Slot::kHead, Slot::kHair, Slot::kLongHair, Slot::kCirclet, Slot::kEars }) {
					const auto& obj = biped->objects[slot];
					if (obj.partClone && !(obj.addon && obj.addon->HasPartOf(RE::BGSBipedObjectForm::BipedObjectSlot::kBody))) {
						hide(obj.partClone.get());
					}
				}
			}
			std::function<void(RE::NiAVObject*)> visit = [&](RE::NiAVObject* a_obj) {
				const std::string_view name = a_obj->name.c_str();
				if (netimmerse_cast<RE::BSFaceGenNiNode*>(a_obj)) {
					hide(a_obj);
					return;
				}
				if (name.starts_with("NPC Head")) {
					// Everything hung on the head bone (it's a bone: its own children only).
					if (auto* node = a_obj->AsNode()) {
						for (auto& child : node->GetChildren()) {
							if (child && !child->AsNode()) {
								hide(child.get());
							} else if (child) {
								const std::string_view cn = child->name.c_str();
								if (!cn.starts_with("NPC ") && !cn.starts_with("Camera")) {
									hide(child.get());
								}
							}
						}
					}
					return;
				}
				if (auto* node = a_obj->AsNode()) {
					for (auto& child : node->GetChildren()) {
						if (child) {
							visit(child.get());
						}
					}
				}
			};
			visit(root);
			using Slot = RE::BGSBipedObjectForm::BipedObjectSlot;
			for (const auto slot : { Slot::kHead, Slot::kHair, Slot::kLongHair, Slot::kCirclet, Slot::kEars }) {
				auto* armor = a_player->GetWornArmor(slot);
				if (!armor || armor->GetSlotMask().any(Slot::kBody)) {
					continue;
				}
				for (auto* arma : armor->armorAddons) {
					if (arma) {
						a_player->VisitArmorAddon(armor, arma, [&](bool a_firstPerson, RE::NiAVObject& a_obj) {
							if (!a_firstPerson) {
								hide(&a_obj);
							}
						});
					}
				}
			}
			static bool logged = false;
			if (!logged) {
				logged = true;
				logger::info("whole-body view: {} head pieces hidden for the whole frame", hiddenHead.size());
			}
		}

		// Whatever Skyrim culled of the body (the root, or each mesh) is shown for drawing; the head
		// stays hidden.
		void ShowBody(RE::PlayerCharacter* a_player, bool a_show)
		{
			if (!a_show) {
				for (auto& obj : shownPieces) {
					obj->SetAppCulled(true);
				}
				shownPieces.clear();
				return;
			}
			auto* root = a_player->Get3D(false);
			if (!root) {
				return;
			}
			std::size_t unculled = 0;
			std::function<void(RE::NiAVObject*)> show = [&](RE::NiAVObject* a_obj) {
				if (netimmerse_cast<RE::BSFaceGenNiNode*>(a_obj) || IsHidden(a_obj)) {
					return;
				}
				if (a_obj->GetAppCulled()) {
					a_obj->SetAppCulled(false);
					shownPieces.emplace_back(a_obj);
					++unculled;
				}
				if (auto* node = a_obj->AsNode()) {
					for (auto& child : node->GetChildren()) {
						if (child) {
							show(child.get());
						}
					}
				}
			};
			show(root);
			static bool loggedPieces = false;
			if (!loggedPieces) {
				loggedPieces = true;
				logger::info("whole-body view: {} hidden body pieces shown for drawing", unculled);
			}
		}

		void SetNearDistance(bool a_faith)
		{
			// The whole-body view keeps it further out: Community Shaders' screen-space effects
			// break down with the body right against the camera.
			const float want = view == View::kSkyrimBody ? GetConfig().nearDistanceBody : GetConfig().nearDistance;
			auto*       ini = RE::INISettingCollection::GetSingleton();
			auto*       setting = ini ? ini->GetSetting("fNearDistance:Display") : nullptr;
			if (!setting || want <= 0.0f) {
				return;
			}
			if (a_faith) {
				if (savedNear < 0.0f) {
					savedNear = setting->GetFloat();
					logger::info("near clip distance {} -> {} while Faith is on ({} in the whole-body view)", savedNear, GetConfig().nearDistance,
						GetConfig().nearDistanceBody);
				}
				setting->SetFloat(want);
			} else if (savedNear >= 0.0f) {
				setting->SetFloat(savedNear);
				savedNear = -1.0f;
			}
		}

		void Notify(const char* a_text)
		{
			RE::SendHUDMessage::ShowHUDMessage(a_text);
			logger::info("{}", a_text);
		}

		RE::TESObjectCELL* lastCell = nullptr;
		// Held in the air while the ground under her hasn't streamed in yet (seconds left).
		float holdFor = 0.0f;
		bool  heldThisFall = false;  // once per fall: a real drop into nothing isn't held again

		void RefreshCollision(RE::PlayerCharacter* a_player, bool a_force)
		{
			const auto& cfg = GetConfig();
			const auto  at = a_player->GetPosition();
			const bool  moved = at.GetDistance(collisionAt) > cfg.collisionRadius * 0.25f;
			auto*       cell = a_player->GetParentCell();
			const bool  newCell = cell != lastCell;
			const bool  falling = haveFrame && frame.velocity.z < -700.0f;
			if (!a_force && !moved && !newCell && collisionTimer > 0.0f) {
				return;
			}
			lastCell = cell;
			// Read again sooner while falling fast or held waiting for the ground.
			collisionTimer = falling || holdFor > 0.0f ? 0.1f : cfg.collisionRefresh;
			// Centred a little ahead of where Faith is going, and reaching well below her (a long
			// drop mustn't outrun it): 60 m down, more the faster she falls.
			auto centre = haveFrame ? P(frame.feet) : at;
			float down = 4200.0f;
			if (haveFrame) {
				centre.x += frame.velocity.x * 0.4f;
				centre.y += frame.velocity.y * 0.4f;
				down += std::max(0.0f, -frame.velocity.z) * 1.5f;
			}
			if (Collision::Harvest(centre, cfg.collisionRadius, cfg.collisionHeight, down, tris)) {
				faith_set_world(faith, tris.data(), static_cast<std::uint32_t>(tris.size() / 9));
				collisionAt = centre;
			}
		}

		void Enable(RE::PlayerCharacter* a_player)
		{
			if (!faith) {
				return;
			}
			active = true;
			haveFrame = false;
			RefreshCollision(a_player, true);
			faith_teleport(faith, { a_player->GetPositionX(), a_player->GetPositionY(), a_player->GetPositionZ() }, a_player->GetAngleZ());
			Input::SetCapturing(true);
			if (faith_animated(faith)) {
				if (!body.tried && GetConfig().thirdPersonBody) {
					body.Bind(faith, a_player, false);
				}
				if (!arms.tried) {
					arms.Bind(faith, a_player, true);
				}
			}
			Notify("Faith: on");
		}

		void StopCourse(RE::PlayerCharacter* a_player);

		void Disable(RE::PlayerCharacter* a_player)
		{
			StopCourse(a_player);
			active = false;
			Input::SetCapturing(false);
			HideFirstPerson(a_player, false);
			SetNearDistance(false);
			HideHead(a_player, false);
			haveGround = false;
			if (forcedThirdPerson) {
				if (auto* camera = RE::PlayerCamera::GetSingleton()) {
					camera->ForceFirstPerson();
				}
				forcedThirdPerson = false;
			}
			if (a_player) {
				if (auto* controller = a_player->GetCharController()) {
					controller->SetLinearVelocityImpl(RE::hkVector4(0.0f, 0.0f, 0.0f, 0.0f));
				}
			}
			Notify("Faith: off");
		}

		// Put the player on one of the app's training courses, built high above where they are.
		void StartCourse(RE::PlayerCharacter* a_player, int a_map)
		{
			if (!faith) {
				return;
			}
			if (!active) {
				Enable(a_player);
			}
			if (!onCourse) {
				returnPos = a_player->GetPosition();
				returnHeading = a_player->GetAngleZ();
			}
			const RE::NiPoint3 anchor = returnPos + RE::NiPoint3{ 0.0f, 0.0f, GetConfig().courseHeight };
			if (!faith_course_start(faith, static_cast<std::uint32_t>(a_map), { anchor.x, anchor.y, anchor.z })) {
				Notify("Faith: couldn't start that course");
				return;
			}
			onCourse = true;
			Input::SetCourse(true);
			holdFor = 0.0f;
			heldThisFall = false;
			haveGround = false;
			FaithCourse status{};
			faith_course_status(faith, &status);
			courseSeq = status.message_seq;
			const auto none = Input::Take(0.0f);
			faith_step(faith, 0.0f, &none, &frame);
			haveFrame = true;
			a_player->SetPosition(P(frame.feet), true);
			logger::info("course '{}' at ({:.0f} {:.0f} {:.0f})", faith_course_name(faith), anchor.x, anchor.y, anchor.z);
			Notify(std::format("Faith: the {} course{}", faith_course_name(faith), GetConfig().respawnKey == 0x13 ? ". R goes back to the checkpoint." : "").c_str());
		}

		void StopCourse(RE::PlayerCharacter* a_player)
		{
			if (!onCourse) {
				return;
			}
			onCourse = false;
			Input::SetCourse(false);
			haveGround = false;
			if (faith) {
				faith_course_stop(faith);
				if (a_player) {
					a_player->SetPosition(returnPos, true);
					if (active) {
						RefreshCollision(a_player, true);
						faith_teleport(faith, { returnPos.x, returnPos.y, returnPos.z }, returnHeading);
						const auto none = Input::Take(0.0f);
						faith_step(faith, 0.0f, &none, &frame);
					}
				}
			}
			Notify("Faith: back from the course");
		}

		void PerFrame(RE::PlayerCharacter* a_player, float a_delta)
		{
			if (wantEnable && a_player->Get3D(false)) {
				wantEnable = false;
				Enable(a_player);
			}
			if (Input::TakeToggle() || menuToggle.exchange(false)) {
				if (active) {
					Disable(a_player);
				} else {
					Enable(a_player);
				}
			}
			const int  wanted = menuView.exchange(-1);
			const bool cycle = Input::TakeViewmodelToggle();
			if (cycle || wanted >= 0) {
				if (wanted >= 0) {
					view = static_cast<View>(std::clamp(wanted, 0, 2));
					if (view == View::kSkyrimBody && body.id < 0) {
						view = View::kSkyrimArms;
					}
				} else {
					switch (view) {
					case View::kFaith:
						view = GetConfig().skyrimBody && body.id >= 0 ? View::kSkyrimBody : View::kSkyrimArms;
						break;
					case View::kSkyrimBody:
						view = View::kSkyrimArms;
						break;
					default:
						view = View::kFaith;
						break;
					}
				}
				Viewmodel::SetVisible(view == View::kFaith);
				// Skyrim draws its body in third person: the whole-body view uses its third-person
				// camera, put at Faith's eyes.
				if (auto* camera = RE::PlayerCamera::GetSingleton()) {
					if (view == View::kSkyrimBody && active) {
						camera->ForceThirdPerson();
						forcedThirdPerson = true;
					} else if (forcedThirdPerson) {
						camera->ForceFirstPerson();
						forcedThirdPerson = false;
					}
				}
				Notify(view == View::kFaith ? "Faith: her own body" : view == View::kSkyrimBody ? "Faith: Skyrim's body" : "Faith: Skyrim's arms");
			}
			if (const auto walk = Input::TakeWalkChange(); walk && active) {
				Notify(*walk ? "Faith: walking" : "Faith: running");
			}
			const int course = menuCourse.exchange(kNoRequest);
			if (course == kLeave) {
				StopCourse(a_player);
			} else if (course >= 0) {
				StartCourse(a_player, course);
			}
			const int  respawnTo = menuRespawn.exchange(kNoRequest);
			const bool respawnKey = Input::TakeRespawn();
			if (onCourse && faith && (respawnKey || respawnTo != kNoRequest)) {
				faith_course_respawn(faith, respawnKey ? -1 : respawnTo);
			}
			if (Input::TakeSurvey()) {
				Notify("Faith: surveying the collision around you...");
				const auto path = Collision::Survey(30000.0f, 12000.0f);
				Notify(path.empty() ? "Faith: survey failed (see the log)" : "Faith: survey saved");
			}
			const bool idleKey = Input::TakeIdle();
			if ((idleKey || menuIdle.exchange(false)) && faith) {
				if (!active) {
					Notify("Faith: switch her on first");
				} else if (!faith_play_idle(faith)) {
					Notify("Faith: stand still on the ground for an idle");
				}
			}
			// The volume from the settings page, as it changes.
			static float volume = -1.0f;
			if (faith && volume != GetConfig().soundVolume) {
				volume = GetConfig().soundVolume;
				faith_sound_volume(faith, volume);
			}
			const bool quiet = !active || MenuOpen() || SkyrimBusy(a_player);
			if (faith) {
				faith_sound_pause(faith, quiet);
			}
			if (!active || !faith) {
				return;
			}
			// Skyrim's arms hide while Faith's own are drawn (checked every frame: Skyrim rebuilds
			// its first-person model when you equip things).
			HideFirstPerson(a_player, false);
			if (DrawingFaith(a_player) && Viewmodel::Ready()) {
				HideFirstPerson(a_player, true);
			} else if (ShowingSkyrimBody(a_player)) {
				HideFirstPerson(a_player, true, false);  // the body holds its own weapons
			}
			SetNearDistance(true);
			HideHead(a_player, ShowingSkyrimBody(a_player));
			if (MenuOpen() || SkyrimBusy(a_player)) {
				Input::Take(0.0f);  // drop what was pressed meanwhile
				return;
			}
			collisionTimer -= a_delta;
			if (!onCourse) {
				RefreshCollision(a_player, false);
			}

			// Nothing under her yet (a cell still streaming in, a load): hold her where she is for a
			// moment, reading the collision again, rather than let her drop out of the world.
			if (onCourse) {
				holdFor = 0.0f;  // the course is all there is: nothing to wait for
			} else if (haveFrame && !frame.on_ground) {
				const bool ground = Collision::GroundUnder(tris, P(frame.feet), 4200.0f + std::max(0.0f, -frame.velocity.z) * 1.5f);
				if (!ground && holdFor <= 0.0f && haveGround && !heldThisFall) {
					holdFor = 3.0f;
					heldThisFall = true;
					logger::info("nothing under Faith at ({:.0f} {:.0f} {:.0f}) yet: holding her while it loads", frame.feet.x, frame.feet.y, frame.feet.z);
				}
				if (ground) {
					holdFor = 0.0f;
				}
			} else {
				holdFor = 0.0f;
				heldThisFall = false;
			}
			if (holdFor > 0.0f) {
				holdFor -= a_delta;
				RefreshCollision(a_player, true);
				Input::Take(0.0f);
				a_player->SetPosition(P(frame.feet), true);
				return;
			}
			const auto input = Input::Take(GetConfig().mouseSensitivity);
			faith_step(faith, a_delta, &input, &frame);
			haveFrame = true;

			// The safety net: dropped 40 m below where she last stood (through a hole in the
			// collision), she's put back there.
			if (onCourse) {
				// Checkpoints and finishing, as the app shows them.
				FaithCourse status{};
				if (!faith_course_status(faith, &status)) {
					onCourse = false;
					Input::SetCourse(false);
				} else if (status.message_seq != courseSeq) {
					courseSeq = status.message_seq;
					Notify(faith_course_message(faith));
				}
			} else if (frame.on_ground) {
				lastGround = P(frame.feet);
				haveGround = true;
			} else if (haveGround && frame.feet.z < lastGround.z - 2800.0f) {
				logger::warn("Faith fell through the world at ({:.0f} {:.0f} {:.0f}); back to ({:.0f} {:.0f} {:.0f})", frame.feet.x, frame.feet.y, frame.feet.z,
					lastGround.x, lastGround.y, lastGround.z);
				RefreshCollision(a_player, true);
				faith_teleport(faith, { lastGround.x, lastGround.y, lastGround.z + 5.0f }, frame.heading);
				const auto again = Input::Take(0.0f);
				faith_step(faith, 0.0f, &again, &frame);
			}
			const auto feet = P(frame.feet);
			a_player->SetPosition(feet, true);
			if (auto* controller = a_player->GetCharController()) {
				// Faith moves the player: Skyrim keeps no momentum or fall damage of its own.
				controller->SetLinearVelocityImpl(RE::hkVector4(0.0f, 0.0f, 0.0f, 0.0f));
				controller->fallStartHeight = feet.z;
				controller->fallTime = 0.0f;
				if (frame.on_ground) {
					controller->context.currentState = RE::hkpCharacterStateType::kOnGround;
					controller->flags.set(RE::CHARACTER_FLAGS::kSupport);
				} else {
					controller->context.currentState = RE::hkpCharacterStateType::kInAir;
				}
			}
			// Skyrim's view follows Faith's look (its camera, AI and aiming read these); the
			// body faces where Faith's does.
			a_player->data.angle.z = frame.heading;
			a_player->data.angle.x = -frame.pitch;

			static float logTimer = 0.0f;
			logTimer -= a_delta;
			if (frame.events && logTimer <= 0.0f) {
				logTimer = 0.5f;
				logger::info("{} / {} at ({:.0f} {:.0f} {:.0f})", faith_state_name(faith), faith_anim_name(faith), feet.x, feet.y, feet.z);
			}
		}

		void ApplyPose(RE::PlayerCharacter* a_player)
		{
			if (!active || !haveFrame || !faith || !frame.animated || SkyrimBusy(a_player)) {
				return;
			}
			if (body.id >= 0) {
				// Seen from her camera, the body's shoulders sit where Faith's do against it.
				body.Apply(faith, a_player->Get3D(false), ShowingSkyrimBody(a_player) ? FAITH_POSE_PIN_HANDS : 0u);
			}
			if (arms.id >= 0) {
				// With her own body drawn, Skyrim's hidden hands go exactly onto hers, so what they
				// hold is in her grip.
				arms.Apply(faith, a_player->Get3D(true), DrawingFaith(a_player) && Viewmodel::Ready() ? FAITH_POSE_PIN_HANDS : 0u);
			}
		}

		// Where Skyrim's camera goes: Faith's eye; in the whole-body view a little ahead of it (and
		// more as she looks down), so it's out in front of the body's neck, collar and chest
		// rather than inside them.
		RE::NiPoint3 Eye()
		{
			auto eye = P(frame.cam_pos);
			if (view == View::kSkyrimBody) {
				const auto& cfg = GetConfig();
				float       t = std::clamp((-frame.pitch - 0.35f) / 0.85f, 0.0f, 1.0f);
				t = t * t * (3.0f - 2.0f * t);
				const float ahead = cfg.bodyCameraForward + cfg.bodyCameraForwardDown * t;
				eye.x += std::sin(frame.heading) * ahead;
				eye.y += std::cos(frame.heading) * ahead;
			}
			return eye;
		}

		// First person: Faith's camera (her camera bone, look and screen shake) on Skyrim's.
		void ApplyCamera(RE::PlayerCamera* a_camera)
		{
			if (!active || !haveFrame || !a_camera->cameraRoot || !FaithCamera(a_camera)) {
				return;
			}
			auto* player = RE::PlayerCharacter::GetSingleton();
			if (!player || SkyrimBusy(player)) {
				return;
			}
			auto*       root = a_camera->cameraRoot.get();
			const auto& R = root->world.rotate;
			if (!axesKnown && !axesRejected) {
				// Skyrim built this from the angles we gave the player: compare it to that look.
				const float h = frame.heading, p = frame.pitch;
				const RE::NiPoint3 f{ std::sin(h) * std::cos(p), std::cos(h) * std::cos(p), std::sin(p) };
				const RE::NiPoint3 r{ std::cos(h), -std::sin(h), 0.0f };
				const RE::NiPoint3 u = r.Cross(f);
				const std::array<RE::NiPoint3, 6> cand{ f, f * -1.0f, u, u * -1.0f, r, r * -1.0f };
				for (int c = 0; c < 3; ++c) {
					int   best = -1;
					float bestAngle = 1e9f;
					for (int k = 0; k < 6; ++k) {
						const float a = AngleDeg(Col(R, c), cand[k]);
						if (a < bestAngle) {
							bestAngle = a;
							best = k;
						}
					}
					if (bestAngle < 6.0f) {
						++axisVotes[c][best];
					}
				}
				if (++axisSamples >= 120) {
					bool                ok = true;
					std::array<bool, 3> used{};
					for (int c = 0; c < 3; ++c) {
						const auto it = std::ranges::max_element(axisVotes[c]);
						const int  k = static_cast<int>(it - axisVotes[c].begin());
						ok &= *it > axisSamples * 6 / 10 && !used[k / 2];
						used[k / 2] = true;
						axisMap[c] = k;
					}
					static constexpr const char* kNames[6] = { "+forward", "-forward", "+up", "-up", "+right", "-right" };
					axesKnown = ok;
					axesRejected = !ok;
					logger::info("camera root axes: {} {} {} -> {}", kNames[axisMap[0]], kNames[axisMap[1]], kNames[axisMap[2]],
						ok ? "Faith's camera turns it" : "inconsistent; only placing it");
				}
			}
			if (axesKnown) {
				const RE::NiPoint3 f = P(frame.cam_forward), u = P(frame.cam_up), r = P(frame.cam_right);
				const std::array<RE::NiPoint3, 6> axes{ f, f * -1.0f, u, u * -1.0f, r, r * -1.0f };
				RE::NiMatrix3 m;
				for (int c = 0; c < 3; ++c) {
					const auto& v = axes[axisMap[c]];
					m.entry[0][c] = v.x;
					m.entry[1][c] = v.y;
					m.entry[2][c] = v.z;
				}
				// In its parent's frame, whatever that is.
				root->local.rotate = root->parent ? root->parent->world.rotate.Transpose() * m : m;
				root->world.rotate = m;
			}
			static bool loggedParent = false;
			if (!loggedParent) {
				loggedParent = true;
				const auto* parent = root->parent;
				logger::info("camera root '{}' hangs from '{}' at ({:.0f} {:.0f} {:.0f})", root->name.c_str(), parent ? parent->name.c_str() : "(none)",
					parent ? parent->world.translate.x : 0.0f, parent ? parent->world.translate.y : 0.0f, parent ? parent->world.translate.z : 0.0f);
			}
			const auto eye = Eye();
			if (const auto* parent = root->parent) {
				const auto& pw = parent->world;
				root->local.translate = (pw.rotate.Transpose() * (eye - pw.translate)) / (pw.scale != 0.0f ? pw.scale : 1.0f);
			} else {
				root->local.translate = eye;
			}
			root->world.translate = eye;
			a_camera->GetRuntimeData2().pos = eye;
			if (auto* sky = RE::Sky::GetSingleton(); sky && sky->root) {
				sky->root->local.translate = eye;
				sky->root->world.translate = eye;
			}
			RE::NiUpdateData update{};
			root->UpdateDownwardPass(update, 0);
			lastCamRot = root->world.rotate;
			lastCamSet = axesKnown;
		}

		// While Faith's own body is drawn, Skyrim's first-person field of view (what its hidden hands
		// hold is drawn with it) follows her arms'; put back after.
		float savedFirstPersonFov = -1.0f;
		void MatchFirstPersonFov(RE::PlayerCharacter* a_player, RE::PlayerCamera* a_camera)
		{
			auto& data = a_camera->GetRuntimeData2();
			if (DrawingFaith(a_player) && Viewmodel::Ready()) {
				if (savedFirstPersonFov < 0.0f) {
					savedFirstPersonFov = data.firstPersonFOV;
				}
				data.firstPersonFOV = Viewmodel::SkyrimFovFor(frame, data.worldFOV);
			} else if (savedFirstPersonFov >= 0.0f) {
				data.firstPersonFOV = savedFirstPersonFov;
				savedFirstPersonFov = -1.0f;
			}
		}

		// Right before Skyrim draws the world: did anything change our pose or camera since we
		// set them? Then set them once more, as the last word.
		void BeforeRender()
		{
			auto* player = RE::PlayerCharacter::GetSingleton();
			auto* camera = RE::PlayerCamera::GetSingleton();
			if (player && camera) {
				MatchFirstPersonFov(player, camera);  // also puts Skyrim's back once Faith is off
				// Skyrim fades the player out when its third-person camera comes close: not when
				// that camera is Faith's eyes.
				if (ShowingSkyrimBody(player)) {
					if (auto* root = player->Get3D(false); root && root->AsFadeNode()) {
						root->AsFadeNode()->GetRuntimeData().currentFade = 1.0f;
					}
				}
			}
			if (!active || !haveFrame || !player || !camera) {
				return;
			}
			++diagFrames;
			if (arms.id >= 0 && arms.Changed()) {
				++diagPoseLost;
			}
			if (lastCamSet && camera->cameraRoot && camera->IsInFirstPerson()) {
				const auto& R = camera->cameraRoot->world.rotate;
				float       d = 0.0f;
				for (int c = 0; c < 3; ++c) {
					d = std::max(d, AngleDeg(Col(R, c), Col(lastCamRot, c)));
				}
				if (d > 0.5f) {
					++diagCamLost;
				}
			}
			ApplyCamera(camera);
			ApplyPose(player);
			if (camera->IsInFirstPerson()) {
				if (auto* drawn = RE::Main::WorldRootCamera()) {
					// NiCamera: column 0 looks, 1 is up, 2 right.
					diagCamOff = std::max(diagCamOff, drawn->world.translate.GetDistance(P(frame.cam_pos)));
					diagCamAngle = std::max(diagCamAngle, AngleDeg(Col(drawn->world.rotate, 0), P(frame.cam_forward)));
				}
				const float h = frame.heading, p = frame.pitch;
				const RE::NiPoint3 look{ std::sin(h) * std::cos(p), std::cos(h) * std::cos(p), std::sin(p) };
				diagAnimTurn = std::max(diagAnimTurn, AngleDeg(look, P(frame.cam_forward)));
			}
			if (diagFrames >= 300) {
				logger::info("before drawing, over {} frames: arms pose changed by Skyrim in {}, camera turned by Skyrim in {}; drawn camera off Faith's by up to {:.1f} units / {:.1f} deg; her camera animation turns the view up to {:.1f} deg from the plain look",
					diagFrames, diagPoseLost, diagCamLost, diagCamOff, diagCamAngle, diagAnimTurn);
				diagFrames = diagPoseLost = diagCamLost = 0;
				diagCamOff = diagCamAngle = diagAnimTurn = 0.0f;
			}
		}

		// Faith's body and the speed blur, into the world or late (before the HUD).
		void DrawFaith(bool a_late)
		{
			auto* player = RE::PlayerCharacter::GetSingleton();
			auto* camera = RE::PlayerCamera::GetSingleton();
			if (!player || !camera) {
				return;
			}
			if (onCourse && active && !SkyrimBusy(player)) {
				Viewmodel::DrawCourse(faith, a_late);
			}
			if (DrawingFaith(player)) {
				Viewmodel::Draw(faith, frame, camera->GetRuntimeData2().worldFOV, a_late);
			}
			// Mirror's Edge's speed blur, whichever body shows.
			if (active && haveFrame && GetConfig().speedBlur && !SkyrimBusy(player) && FaithCamera(camera)) {
				Viewmodel::SpeedBlur(frame.speed_blur, a_late);
			}
		}

		// The HUD is about to draw: everything else in the frame is done.
		struct HUDDisplayHook
		{
			static void thunk(RE::IMenu* a_this)
			{
				if (DrawLate() && !MenuOpen()) {
					DrawFaith(true);
				}
				func(a_this);
			}
			static inline REL::Relocation<decltype(thunk)> func;
		};

		struct RenderWorldHook
		{
			static void thunk(bool a_unk)
			{
				BeforeRender();
				auto* p = RE::PlayerCharacter::GetSingleton();
				const bool body3p = p && ShowingSkyrimBody(p);
				if (body3p) {
					ShowBody(p, true);
				}
				func(a_unk);
				if (body3p) {
					ShowBody(p, false);
				}
				// The world is drawn and not yet tone mapped: Faith's own body goes in now (unless
				// it's drawn late).
				if (!DrawLate()) {
					DrawFaith(false);
				}
			}
			static inline REL::Relocation<decltype(thunk)> func;
		};

		// BSShadowDirectionalLight::Render: the sun's shadow maps are complete when it returns.
		struct SunShadowRenderHook
		{
			static void thunk(RE::BSShadowLight* a_this, std::uint32_t& a_index)
			{
				func(a_this, a_index);
				if (active && haveFrame) {
					Viewmodel::CaptureSunShadows(a_this);
				}
			}
			static inline REL::Relocation<decltype(thunk)> func;
		};

		struct PlayerUpdateHook
		{
			static void thunk(RE::PlayerCharacter* a_this, float a_delta)
			{
				func(a_this, a_delta);
				PerFrame(a_this, a_delta);
			}
			static inline REL::Relocation<decltype(thunk)> func;
		};

		struct PlayerAnimationHook
		{
			static void thunk(RE::PlayerCharacter* a_this, float a_delta)
			{
				func(a_this, a_delta);
				ApplyPose(a_this);
			}
			static inline REL::Relocation<decltype(thunk)> func;
		};

		struct PlayerCameraUpdateHook
		{
			static void thunk(RE::PlayerCamera* a_this)
			{
				func(a_this);
				ApplyCamera(a_this);
				if (auto* player = RE::PlayerCharacter::GetSingleton()) {
					ApplyPose(player);  // after the first-person model was placed for this frame
				}
			}
			static inline REL::Relocation<decltype(thunk)> func;
		};

		// First-person camera position = Faith's eye.
		struct FirstPersonTranslationHook
		{
			static void thunk(RE::TESCameraState* a_this, RE::NiPoint3& a_out)
			{
				func(a_this, a_out);
				if (active && haveFrame) {
					a_out = Eye();
				}
			}
			static inline REL::Relocation<decltype(thunk)> func;
		};
	}

	void Install()
	{
		const auto& cfg = GetConfig();
		view = cfg.faithViewmodel ? View::kFaith : cfg.skyrimBody ? View::kSkyrimBody : View::kSkyrimArms;
		Viewmodel::SetVisible(view == View::kFaith);
		faith = faith_create(cfg.mirrorsEdgeDir.empty() ? nullptr : cfg.mirrorsEdgeDir.c_str(), 0.0f);
		if (!faith) {
			logger::error("couldn't start Faith: {}", faith_last_error());
			return;
		}
		faith_sound_volume(faith, cfg.soundVolume);
		faith_sound_pause(faith, 1);
		logger::info("Faith's sounds: {} cues{}", faith_sound_cues(faith), faith_sound_cues(faith) ? "" : " (none: no Mirror's Edge sound packages or no sound device)");
		if (faith_animated(faith)) {
			logger::info("Mirror's Edge animations loaded");
		} else {
			logger::warn("no Mirror's Edge install found ({}); Faith moves but isn't animated. Set sMirrorsEdgeDir in FaithSkyrim.ini", faith_last_error());
		}

		REL::Relocation<std::uintptr_t> playerVtbl{ RE::VTABLE_PlayerCharacter[0] };
		PlayerUpdateHook::func = playerVtbl.write_vfunc(0xAD, PlayerUpdateHook::thunk);
		PlayerAnimationHook::func = playerVtbl.write_vfunc(0x7D, PlayerAnimationHook::thunk);

		// PlayerCamera::Update is called directly (not through the vtable): hook its call sites.
		{
			const auto  target = REL::Relocation<std::uintptr_t>{ RELOCATION_ID(49852, 50784) }.address();
			const auto  text = REL::Module::get().segment(REL::Segment::textx);
			const auto  base = text.address();
			const auto* code = reinterpret_cast<const std::uint8_t*>(base);
			std::vector<std::uintptr_t> sites;
			for (std::size_t i = 0; i + 5 <= text.size(); ++i) {
				if (code[i] != 0xE8) {
					continue;
				}
				std::int32_t rel;
				std::memcpy(&rel, code + i + 1, 4);
				if (base + i + 5 + static_cast<std::intptr_t>(rel) == target) {
					sites.push_back(base + i);
				}
			}
			auto& trampoline = SKSE::GetTrampoline();
			for (const auto site : sites) {
				PlayerCameraUpdateHook::func = trampoline.write_call<5>(site, PlayerCameraUpdateHook::thunk);
			}
			logger::info("PlayerCamera::Update: hooked {} call site(s)", sites.size());
		}

		// Main::RenderWorld's one call in the frame function (SkyCraft's site for 1.7.104).
		{
			const auto  site = REL::ID(36559).address() + 0x85E;
			const auto* code = reinterpret_cast<const std::uint8_t*>(site);
			std::int32_t rel = 0;
			std::memcpy(&rel, code + 1, 4);
			if (code[0] == 0xE8 && site + 5 + static_cast<std::intptr_t>(rel) == REL::ID(107142).address()) {
				RenderWorldHook::func = SKSE::GetTrampoline().write_call<5>(site, RenderWorldHook::thunk);
				logger::info("hooked before Main::RenderWorld");
			} else {
				logger::warn("Main::RenderWorld's call isn't where expected; the pose is only set in the update hooks");
			}
		}

		REL::Relocation<std::uintptr_t> hudVtbl{ RE::VTABLE_HUDMenu[0] };
		HUDDisplayHook::func = hudVtbl.write_vfunc(0x6, HUDDisplayHook::thunk);
		logger::info("Faith's body is drawn {}", DrawLate() ? "late, before the HUD (Community Shaders is loaded, or iDrawStage=2)" : "into the world");

		REL::Relocation<std::uintptr_t> sunVtbl{ RE::VTABLE_BSShadowDirectionalLight[0] };
		SunShadowRenderHook::func = sunVtbl.write_vfunc(0x0A, SunShadowRenderHook::thunk);

		REL::Relocation<std::uintptr_t> fpVtbl{ RE::VTABLE_FirstPersonState[0] };
		FirstPersonTranslationHook::func = fpVtbl.write_vfunc(0x5, FirstPersonTranslationHook::thunk);

		Input::Install();
		logger::info("Faith installed: press {:#x} in game to switch her on and off", cfg.toggleKey);
	}

	void OnGameLoaded()
	{
		// A new 3D: bind the skeletons again when Faith next switches on.
		body.Reset();
		arms.Reset();
		haveFrame = false;
		if (onCourse && faith) {
			faith_course_stop(faith);
		}
		onCourse = false;
		Input::SetCourse(false);
		if (loadedReturn) {
			// Saved on a training course: back to where it was started from.
			if (auto* player = RE::PlayerCharacter::GetSingleton()) {
				player->SetPosition(loadedReturn->first, true);
				player->data.angle.z = loadedReturn->second;
				logger::info("loaded a save made on a training course: back to ({:.0f} {:.0f} {:.0f})", loadedReturn->first.x, loadedReturn->first.y,
					loadedReturn->first.z);
			}
			loadedReturn.reset();
		}
		if (active) {
			active = false;
			Input::SetCapturing(false);
			wantEnable = true;
		} else if (GetConfig().startEnabled) {
			wantEnable = true;
		}
	}

	void RequestCourse(int a_map) { menuCourse = a_map; }
	void RequestLeaveCourse() { menuCourse = kLeave; }
	void RequestCheckpoint(int a_checkpoint) { menuRespawn = a_checkpoint; }
	bool OnCourse() { return onCourse; }
	::Faith* Handle() { return faith; }

	namespace
	{
		constexpr std::uint32_t kSaveId = 'FRUN', kReturnRecord = 'CRET', kReturnVersion = 1;

		void OnSave(SKSE::SerializationInterface* a_intfc)
		{
			if (!onCourse) {
				return;
			}
			const float data[4] = { returnPos.x, returnPos.y, returnPos.z, returnHeading };
			if (a_intfc->OpenRecord(kReturnRecord, kReturnVersion)) {
				a_intfc->WriteRecordData(data, sizeof(data));
			}
		}

		void OnLoad(SKSE::SerializationInterface* a_intfc)
		{
			std::uint32_t type = 0, version = 0, length = 0;
			while (a_intfc->GetNextRecordInfo(type, version, length)) {
				float data[4]{};
				if (type == kReturnRecord && version == kReturnVersion && length == sizeof(data) && a_intfc->ReadRecordData(data, sizeof(data)) == sizeof(data)) {
					loadedReturn = std::make_pair(RE::NiPoint3{ data[0], data[1], data[2] }, data[3]);
				}
			}
		}

		void OnRevert(SKSE::SerializationInterface*) { loadedReturn.reset(); }
	}

	void RegisterSaves()
	{
		if (auto* s = SKSE::GetSerializationInterface()) {
			s->SetUniqueID(kSaveId);
			s->SetSaveCallback(OnSave);
			s->SetLoadCallback(OnLoad);
			s->SetRevertCallback(OnRevert);
		}
	}

	bool IsActive() { return active; }
	int  CurrentView() { return static_cast<int>(view); }
	void RequestToggle() { menuToggle = true; }
	void RequestView(int a_view) { menuView = a_view; }
	void RequestIdle() { menuIdle = true; }
}
