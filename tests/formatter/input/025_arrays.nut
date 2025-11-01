function onCombatStarted() {
	local actor = this.getContainer().getActor();
	actor.m.BloodType = this.Const.BloodType.Bones;
	actor.m.Sound[this.Const.Sound.ActorEvent.NoDamageReceived] = [
		"sounds/enemies/ghost_death_01.wav",
		"sounds/enemies/ghost_death_02.wav"
	];
	actor.m.Sound[this.Const.Sound.ActorEvent.DamageReceived] = [
		"sounds/enemies/ghost_death_01.wav",
		"sounds/enemies/ghost_death_02.wav"
	];
	actor.m.Sound[this.Const.Sound.ActorEvent.Death] = [
		"sounds/enemies/ghost_death_01.wav",
		"sounds/enemies/ghost_death_02.wav"
	];
	actor.m.Sound[this.Const.Sound.ActorEvent.Fatigue] = [
		"sounds/enemies/ghastly_touch_01.wav"
	];
	actor.m.Sound[this.Const.Sound.ActorEvent.Flee] = [
		"sounds/enemies/ghastly_touch_01.wav"
	];
	actor.m.Sound[this.Const.Sound.ActorEvent.Idle] = [
		"sounds/enemies/skeleton_idle_01.wav",
		"sounds/enemies/skeleton_idle_02.wav",
		"sounds/enemies/skeleton_idle_03.wav",
		"sounds/enemies/skeleton_idle_04.wav",
		"sounds/enemies/skeleton_idle_05.wav",
		"sounds/enemies/skeleton_idle_06.wav"
	];
}