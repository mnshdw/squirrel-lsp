local ret = [];
if (this.isFullBlessing()) {
	ret.push({
		id = 5,
		type = "text",
		icon = "ui/icons/melee_skill_va11.png",
		text = "[color=" + ::Const.UI.Color.Status + "]Burns[/color] the ground on attack.\nIf the ground is already [color=" + ::Const.UI.Color.Status + "]Burning[/color], doubles the bonus damage and removes the fire."
	});
	ret.push({
		id = 6,
		type = "text",
		icon = "ui/icons/plus.png",
		text = "Grants immunity to [color=" + ::Const.UI.Color.Status + "]Burn[/color]. Standing on fire recovers [color=" + ::Const.UI.Color.PositiveValue + "]" + this.m.HpRecover + "%[/color] HP and [color=" + ::Const.UI.Color.PositiveValue + "]" + this.m.FatigueRecover + "[/color] Fatigue per turn."
	});
}
