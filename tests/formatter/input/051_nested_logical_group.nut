function f() {
	if (!attackCanceled
		&& (Input.key_pressed(Input.KeySpace)
		|| Input.gamepad_button_pressed(Input.GamepadButtonSouth))) {
		attack()
	}

	if (ready && (a || b)) {
		go()
	}
}
