function update(dt) {
	setupOnce()
	//print("FPS: " + System.fps() + ", UPS: " + System.ups() + "\n")

	if (GAME_STATE == GameState.play) {
		local maxCommandNum = 0
		switch (ui.optionsSubState) {
			case 0:
				maxCommandNum = 5
				// Full Screen
				if (ui.commandNum == 0) {
					toggleFullScreen()
				}
				break
			default:
				maxCommandNum = 0
				/* nothing to do here */
				break
		}
	}
	local values = [
		1,
		// the second one
		2,

		// a blank line that sets off a commented group is kept
		3,

		4,
	]
	drawUI(values) // still trails the statement it follows
}
