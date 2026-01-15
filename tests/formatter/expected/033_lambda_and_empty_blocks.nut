::mods_queue(::Legends.ID + "_load_order_fix", [
	">mod_legends", // mods listed here will be forced to load after legends
	"<mod_prepare_carefully",
].reduce(@(p, n) ::format("%s, %s", p, n)), function () {});

// Lambda with expression body starting with ::
local f = @(x) ::GlobalFunc(x);

// Empty function blocks
function empty() {}
local fn = function () {};
q.hook = @(__original) function () {};
