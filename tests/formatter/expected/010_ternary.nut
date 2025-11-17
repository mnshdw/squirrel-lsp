local addEntity = function (_type, _name, _namePlural = null, _icon = null) {
	local index = ::Const.EntityIcon.len();
	::Const.EntityType[_type] <- index;
	::Const.EntityIcon.push(_icon == null ? "skeleton_08_orientation" : _icon);
	::Const.Strings.EntityNamePlural.push(_namePlural == null ? _name : _namePlural);
	::Const.Strings.EntityName.push(_name);
};

local ret = [
	{
		id = 1,
		type = "title",
		text = this.getName() + (isWeakened ? " (Weakened)" : "")
	}
];

_properties.MovementAPCostAdditional += isWeakened
	? this.m.ApCostReductionWeakened
	: this.m.ApCostReductionFull;
_properties.MovementFatigueCostMult *= isWeakened
	? this.m.FatigueReductionWeakened
	: this.m.FatigueReductionFull;

function setGender(_gender = -1) {
	if (_gender == -1) {
		_gender = ::Legends.Mod.ModSettings.getSetting("GenderEquality").getValue() == "Disabled"
			? 0
			: ::Math.rand(0, 1);
	}

	if (_gender != 1) {
		return;
	}

	this.addBackgroundType(this.Const.BackgroundType.Female);
}
