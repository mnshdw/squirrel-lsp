function test(_targetEntity)
{
	if ((_targetEntity.isDying() || _targetEntity.isAlive()) && _targetEntity.getFaction() != this.Const.Faction.Player && _targetEntity.getFaction() != this.Const.Faction.PlayerAnimals)
	{
		return 1;
	}
}
