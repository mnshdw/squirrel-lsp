function risky() {
	try {
		mayThrow()
	} catch (error) {
		this.logError(error)
	}
}
