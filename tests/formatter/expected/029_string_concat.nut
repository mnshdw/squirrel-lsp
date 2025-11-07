function f(x) {
	local _troopEntry = {
		Script = "foo/bar"
	};
	if (::MSU.String.endsWith(_troopEntry.Script, "/" + x)) {
		return false;
	}
}
