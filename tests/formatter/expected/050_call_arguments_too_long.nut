function f() {
	fontMaruMonica.draw(
		frameX + TILE_SIZE / 2,
		frameY + TILE_SIZE / 1.5,
		"Esc / [" + gamepadButtonIDToLabel(Input.GamepadButtonEast) + "] : Back"
	)
	this.inherit("scripts/skills/skill", {
		m = {},

		function create() {
			this.m.Type = this.Const.SkillType.Perk;
		}
	});
	shortCall(a, b)
	oneArgumentThatIsFarTooLongToFitOnASingleLineButHasNowhereToBreak(theOnlyArgumentIsThisOne)
}
