switch(_id)
{
case ::Const.EntityType.ModBossGB:
	return ::Const.FactionType.Barbarians;

case ::Const.EntityType.ModIceGolem:
case ::Const.EntityType.ModBossDrasethis:
case ::Const.EntityType.ModBossIndomitableSnowman:
	return ::Const.FactionType.Beasts;

default:
	return old_getDefaultFaction(_id);
}