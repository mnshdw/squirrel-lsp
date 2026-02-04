function hasDualWeapons(_actor, _type) {
	local items = _actor.getItems();
	local mh = items.getItemAtSlot(::Const.ItemSlot.Mainhand);
	local oh = items.getItemAtSlot(::Const.ItemSlot.Offhand);
	return mh != null
		&& oh != null
		&& ("isWeaponType" in mh)
		&& mh.isWeaponType(_type)
		&& ("isWeaponType" in oh)
		&& oh.isWeaponType(_type);
}
