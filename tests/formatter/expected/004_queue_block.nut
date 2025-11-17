::Const.AI.ParallelizationMode = false;
::Mod_ROTU.ModHook <- ::Hooks.register(::Mod_ROTU.ID, ::Mod_ROTU.Version, ::Mod_ROTU.Name);
::Mod_ROTU.ModHook.require("mod_legends >= 19.2.0", "mod_modern_hooks >= 0.4.0", "mod_msu >= 1.2.7");
::Mod_ROTU.ModHook.conflictWith("mod_more_bandits", "mod_elite_few", "mod_background_perks", "mod_rpgr_parameters", "mod_necro_origin", "mod_LA", "Chirutiru_balance", "mod_Chirutiru_enemies", "zChirutiru_equipment", "fromTheGrave", "mod_immortal_warriors", "mod_item_spawner", "mod_partiesDropNameds", "mod_weapons_updated", "mod_weapons", "mod_scaling_avatar", "mod_reforged", "mod_bro_studio", "mod_RevampedXPSystem", "mod_rpgr_raids", "modMoreArrows", "mod_breditor", "mod_alwaysLootNamedItems", "mod_beast_loot");
::Mod_ROTU.ModHook.queue(">mod_legends", ">mod_msu", ">mod_nggh_magic_concept", ">mod_ACU", ">mod_stronghold", function () {

	::Mod_ROTU.Mod <- ::MSU.Class.Mod(::Mod_ROTU.ID, ::Mod_ROTU.Version, ::Mod_ROTU.Name);

	::HasAC <- ::Hooks.hasMod("mod_ACU");
	::HasMC <- ::Hooks.hasMod("mod_nggh_magic_concept");
	::HasStronghold <- ::Hooks.hasMod("mod_stronghold");
	::HasFB <- ::Hooks.hasMod("mod_fantasybro");
	::HasPoV <- ::Hooks.hasMod("mod_PoV");
	::HasSSU <- ::Hooks.hasMod("mod_sellswords");

	::include("mod_ROTU/load.nut");
	::include("mod_snow_chat_origin/load.nut");
});
