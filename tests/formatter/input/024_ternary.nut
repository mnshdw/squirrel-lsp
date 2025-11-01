function setGender(_gender = -1) {
	if (_gender == -1) {
		_gender = ::Legends.Mod.ModSettings.getSetting("GenderEquality").getValue() == "Disabled" ? 0 : ::Math.rand(0, 1);
	}

	if (_gender != 1) {
		return;
	}

	this.addBackgroundType(this.Const.BackgroundType.Female);
}