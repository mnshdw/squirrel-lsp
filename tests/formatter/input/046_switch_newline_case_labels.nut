function gamepadButtonIDToLabel(buttonID) {
	local label = Input.gamepad_button_label(buttonID)
	switch (label) {
		case Input.GamepadButtonLabelCross:
			return "Cross"
		case Input.GamepadButtonLabelCircle:
			return "Circle"
		default:
			return buttonID.tostring()
	}
}

function checkTile(entity) {
	switch (tempDirection) {
		case "up":
			entityTopRow = (entityTopWorldY - entity.speed) / TILE_SIZE
			tileNum1 = tileManager.mapTileNum[CURRENT_MAP][entityLeftCol][entityTopRow]
			break
		case "down":
			entityBottomRow = (entityBottomWorldY + entity.speed) / TILE_SIZE
			break
		default:
			break
	}
}
