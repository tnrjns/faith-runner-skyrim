#include "Menu.h"
#include "Config.h"
#include "FaithMode.h"

namespace
{
	void SetupLog()
	{
		auto dir = SKSE::log::log_directory();
		if (!dir) {
			return;
		}
		auto path = *dir / "FaithSkyrim.log";
		auto sink = std::make_shared<spdlog::sinks::basic_file_sink_mt>(path.string(), true);
		auto log = std::make_shared<spdlog::logger>("global", std::move(sink));
		log->set_level(spdlog::level::info);
		log->flush_on(spdlog::level::info);
		spdlog::set_default_logger(std::move(log));
		spdlog::set_pattern("[%H:%M:%S.%e] [%l] %v");
	}

	void OnMessage(SKSE::MessagingInterface::Message* a_msg)
	{
		switch (a_msg->type) {
		case SKSE::MessagingInterface::kDataLoaded:
			faith::Install();
			faith::Menu::Register();
			break;
		case SKSE::MessagingInterface::kPostLoadGame:
		case SKSE::MessagingInterface::kNewGame:
			faith::OnGameLoaded();
			break;
		default:
			break;
		}
	}
}

SKSEPluginLoad(const SKSE::LoadInterface* a_skse)
{
	SKSE::Init(a_skse, { .trampoline = true, .trampolineSize = 512 });
	SetupLog();
	logger::info("FaithSkyrim loading (runtime {}, faith api {})", a_skse->RuntimeVersion().string(), faith_api_version());
	if (faith_api_version() != FAITH_API_VERSION) {
		logger::error("faith_ffi is API {}, the plugin was built for {}", faith_api_version(), FAITH_API_VERSION);
		return false;
	}
	faith::LoadConfig();
	faith::RegisterSaves();
	SKSE::GetMessagingInterface()->RegisterListener(OnMessage);
	return true;
}
