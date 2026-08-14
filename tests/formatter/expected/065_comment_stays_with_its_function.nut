this.x <- inherit("scripts/skills/skill", {
	m = {},

	// onAnySkillUsed is the wrong hook here. It runs from buildPropertiesForUse,
	// so it fires on hover. This one runs only on a real execution.
	function onAnySkillExecuted(_skill, _targetTile) {
		foo()
	}

	function undocumented() {
		bar()
	}
});
local opens = {
	// a comment that opens the table takes no blank above it
	function first() {
		baz()
	}
};
