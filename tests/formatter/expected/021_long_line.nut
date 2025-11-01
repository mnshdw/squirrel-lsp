local actor = this.getContainer().getActor();
this.Tactical.spawnIconEffect(
	"status_effect_79",
	actor.getTile(),
	this.Const.Tactical.Settings.SkillIconOffsetX,
	this.Const.Tactical.Settings.SkillIconOffsetY,
	this.Const.Tactical.Settings.SkillIconScale,
	this.Const.Tactical.Settings.SkillIconFadeInDuration,
	this.Const.Tactical.Settings.SkillIconStayDuration,
	this.Const.Tactical.Settings.SkillIconFadeOutDuration,
	this.Const.Tactical.Settings.SkillIconMovement
);
