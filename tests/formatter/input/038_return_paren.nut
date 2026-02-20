::Nggh_MagicConcept.isHexeOrigin <- function () {
	return ("Assets" in ::World)
		&& ::World.Assets != null
		&& ::World.Assets.getOrigin() != null
		&& ::World.Assets.getOrigin().getID() == "scenario.hexe";
}

function foo() {
	return (1 + 2);
}

function bar() {
	throw ("error message");
}
