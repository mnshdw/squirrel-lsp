function tally(rank, experience) {
	local text   =   "XP";
	// fmt: off
	text = text
	     + " ("   + experience
	     + " / "  + ::NF.WeaponXP.Ranks[rank + 1].Experience
	     + ")";
	// fmt: on
	return   text;
}
