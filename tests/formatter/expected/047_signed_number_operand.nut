function move(user) {
	switch (user.direction) {
		case "up":
			nextWorldY = user.getTopY() - 4;
			break;
		case "down":
			nextWorldY = user.getBottomY() + 4;
			break;
		case "left":
			nextWorldX = user.getLeftX() - 4;
			break;
		case "right":
			nextWorldX = user.getRightX() + 4;
			break;
	}

	local width = tiles[index] - 2;
	local height = size - 2.5;
	local offsets = [-4, -2, 0, 2];
	local delta = -4;
	local scaled = -4 * factor;
	setPosition(-4, height);
	local clamped = max(-4, height - 1);
}
