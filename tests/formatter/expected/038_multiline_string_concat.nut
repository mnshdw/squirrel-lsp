function create() {
	this.m.Description = "An old Vatt'ghern has answered the call. "
		+ "Armed with mutagens, hunt the Usurper.\n\n"
		+ "[color=#bcad8c]Vatt'ghern:[/color] Witcher.\n"
		+ "[color=#bcad8c]Alchemy:[/color] Mutagens.\n"
		+ "[color=#bcad8c]Champion:[/color] Tiered system.\n"
		+ "[color=#bcad8c]No Avatar:[/color] No main character.[/p]";

	local x = "hello" + " world";
	local y = a + b + c;
}

// String concat inside parens (e.g. inherit)
this.foo <- this.inherit("scripts/foo", {
	function create() {
		this.m.Description = "[p=c][img]gfx/ui/events/rotu_origin.png[/img][/p]"
			+ "[p]An old Vatt'ghern has answered the call of the Raven god. "
			+ "Armed with mutagens and witcher potions, hunt the Usurper.\n\n"
			+ "[color=#bcad8c]No Avatar:[/color] There is no main character.[/p]";
	}
});
