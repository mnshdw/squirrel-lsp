local addEntity = function(_type, _name, _namePlural = null, _icon = null) {
	local index = ::Const.EntityIcon.len();
	::Const.EntityType[_type] <- index;
	::Const.EntityIcon.push(_icon == null ? "skeleton_08_orientation" : _icon);
	::Const.Strings.EntityNamePlural.push(_namePlural == null ? _name : _namePlural);
	::Const.Strings.EntityName.push(_name);
};
