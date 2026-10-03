#include "Config.h"

#include <SimpleIni.h>

namespace faith
{
	namespace
	{
		Config config;
	}

	const Config& GetConfig() { return config; }
	Config&       EditConfig() { return config; }

	void ResetConfig()
	{
		const auto dir = config.mirrorsEdgeDir;
		config = Config{};
		config.mirrorsEdgeDir = dir;
		SaveConfig();
		logger::info("settings reset to the defaults");
	}

	namespace
	{
		std::string Hex(std::uint32_t a_key) { return std::format("{:#x}", a_key); }
	}

	void SaveConfig()
	{
		// Into the ini as it is (its comments kept), only the values changed.
		CSimpleIniA ini;
		ini.SetUnicode();
		const auto path = L"Data/SKSE/Plugins/FaithSkyrim.ini";
		ini.LoadFile(path);
		ini.SetValue("General", "iToggleKey", Hex(config.toggleKey).c_str());
		ini.SetBoolValue("General", "bStartEnabled", config.startEnabled);
		ini.SetDoubleValue("General", "fMouseSensitivity", config.mouseSensitivity);
		ini.SetBoolValue("General", "bThirdPersonBody", config.thirdPersonBody);
		ini.SetBoolValue("General", "bFaithViewmodel", config.faithViewmodel);
		ini.SetValue("General", "iViewmodelKey", Hex(config.viewmodelKey).c_str());
		ini.SetLongValue("General", "iDrawStage", config.drawStage);
		ini.SetBoolValue("General", "bSpeedBlur", config.speedBlur);
		ini.SetValue("General", "iSurveyKey", Hex(config.surveyKey).c_str());
		ini.SetValue("General", "iIdleKey", Hex(config.idleKey).c_str());
		ini.SetValue("Course", "iRespawnKey", Hex(config.respawnKey).c_str());
		ini.SetBoolValue("Combat", "bMeleeHits", config.meleeHits);
		ini.SetBoolValue("General", "bWorldFixtures", config.worldFixtures);
		ini.SetBoolValue("General", "bStamina", config.stamina);
		ini.SetBoolValue("General", "bHeldInGrip", config.heldInGrip);
		ini.SetBoolValue("General", "bBodyScreenMatch", config.bodyScreenMatch);
		ini.SetDoubleValue("General", "fHeldRange", config.heldRange);
		ini.SetDoubleValue("Combat", "fMeleeDamageMult", config.meleeDamageMult);
		ini.SetValue("Combat", "iTakedownKey", Hex(config.takedownKey).c_str());
		ini.SetBoolValue("Combat", "bTakedownKills", config.takedownKills);
		ini.SetValue("General", "iWalkKey", Hex(config.walkKey).c_str());
		ini.SetDoubleValue("General", "fWalkStick", config.walkStick);
		ini.SetDoubleValue("General", "fBodyCameraForward", config.bodyCameraForward);
		ini.SetDoubleValue("General", "fBodyCameraForwardDown", config.bodyCameraForwardDown);
		ini.SetDoubleValue("Course", "fHeight", config.courseHeight);
		ini.SetDoubleValue("General", "fNearDistance", config.nearDistance);
		ini.SetDoubleValue("General", "fNearDistanceBody", config.nearDistanceBody);
		ini.SetDoubleValue("General", "fSoundVolume", config.soundVolume);
		ini.SetDoubleValue("Collision", "fRadius", config.collisionRadius);
		ini.SetDoubleValue("Collision", "fHeight", config.collisionHeight);
		ini.SetDoubleValue("Collision", "fRefreshSeconds", config.collisionRefresh);
		if (ini.SaveFile(path) < 0) {
			logger::error("couldn't save FaithSkyrim.ini");
		} else {
			logger::info("settings saved to FaithSkyrim.ini");
		}
	}

	void LoadConfig()
	{
		CSimpleIniA ini;
		ini.SetUnicode();
		const auto path = L"Data/SKSE/Plugins/FaithSkyrim.ini";
		if (ini.LoadFile(path) < 0) {
			logger::info("no FaithSkyrim.ini; using the defaults");
			return;
		}
		config.mirrorsEdgeDir = ini.GetValue("General", "sMirrorsEdgeDir", "");
		config.toggleKey = static_cast<std::uint32_t>(ini.GetLongValue("General", "iToggleKey", static_cast<long>(config.toggleKey)));
		config.startEnabled = ini.GetBoolValue("General", "bStartEnabled", config.startEnabled);
		config.mouseSensitivity = static_cast<float>(ini.GetDoubleValue("General", "fMouseSensitivity", config.mouseSensitivity));
		config.thirdPersonBody = ini.GetBoolValue("General", "bThirdPersonBody", config.thirdPersonBody);
		config.faithViewmodel = ini.GetBoolValue("General", "bFaithViewmodel", config.faithViewmodel);
		config.surveyKey = static_cast<std::uint32_t>(ini.GetLongValue("General", "iSurveyKey", static_cast<long>(config.surveyKey)));
		config.idleKey = static_cast<std::uint32_t>(ini.GetLongValue("General", "iIdleKey", static_cast<long>(config.idleKey)));
		config.walkKey = static_cast<std::uint32_t>(ini.GetLongValue("General", "iWalkKey", static_cast<long>(config.walkKey)));
		config.walkStick = static_cast<float>(ini.GetDoubleValue("General", "fWalkStick", config.walkStick));
		config.bodyCameraForward = static_cast<float>(ini.GetDoubleValue("General", "fBodyCameraForward", config.bodyCameraForward));
		config.bodyCameraForwardDown = static_cast<float>(ini.GetDoubleValue("General", "fBodyCameraForwardDown", config.bodyCameraForwardDown));
		config.worldFixtures = ini.GetBoolValue("General", "bWorldFixtures", config.worldFixtures);
		config.bodyScreenMatch = ini.GetBoolValue("General", "bBodyScreenMatch", config.bodyScreenMatch);
		config.heldInGrip = ini.GetBoolValue("General", "bHeldInGrip", config.heldInGrip);
		config.heldRange = static_cast<float>(ini.GetDoubleValue("General", "fHeldRange", config.heldRange));
		config.stamina = ini.GetBoolValue("General", "bStamina", config.stamina);
		config.meleeHits = ini.GetBoolValue("Combat", "bMeleeHits", config.meleeHits);
		config.meleeDamageMult = static_cast<float>(ini.GetDoubleValue("Combat", "fMeleeDamageMult", config.meleeDamageMult));
		config.takedownKey = static_cast<std::uint32_t>(ini.GetLongValue("Combat", "iTakedownKey", static_cast<long>(config.takedownKey)));
		config.takedownKills = ini.GetBoolValue("Combat", "bTakedownKills", config.takedownKills);
		config.respawnKey = static_cast<std::uint32_t>(ini.GetLongValue("Course", "iRespawnKey", static_cast<long>(config.respawnKey)));
		config.courseHeight = static_cast<float>(ini.GetDoubleValue("Course", "fHeight", config.courseHeight));
		config.nearDistanceBody = static_cast<float>(ini.GetDoubleValue("General", "fNearDistanceBody", config.nearDistanceBody));
		config.nearDistance = static_cast<float>(ini.GetDoubleValue("General", "fNearDistance", config.nearDistance));
		config.drawStage = static_cast<int>(ini.GetLongValue("General", "iDrawStage", config.drawStage));
		config.speedBlur = ini.GetBoolValue("General", "bSpeedBlur", config.speedBlur);
		config.soundVolume = static_cast<float>(ini.GetDoubleValue("General", "fSoundVolume", config.soundVolume));
		config.viewmodelKey = static_cast<std::uint32_t>(ini.GetLongValue("General", "iViewmodelKey", static_cast<long>(config.viewmodelKey)));
		config.collisionRadius = static_cast<float>(ini.GetDoubleValue("Collision", "fRadius", config.collisionRadius));
		config.collisionHeight = static_cast<float>(ini.GetDoubleValue("Collision", "fHeight", config.collisionHeight));
		config.collisionRefresh = static_cast<float>(ini.GetDoubleValue("Collision", "fRefreshSeconds", config.collisionRefresh));
		logger::info("config: Mirror's Edge '{}', toggle key {:#x}, sensitivity {}, collision radius {} height {} every {} s",
			config.mirrorsEdgeDir, config.toggleKey, config.mouseSensitivity, config.collisionRadius, config.collisionHeight, config.collisionRefresh);
	}
}
