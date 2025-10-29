if (distance <= 3 && t.isAlliedWith(actor)) {
	switch (distance) {
		case 1:
			expertise = expertise + 2.0 / this.m.Difficulty;
			break;
		case 2:
			expertise = expertise + 1.0 / this.m.Difficulty;
			break;
		case 3:
			expertise = expertise + 0.5 / this.m.Difficulty;
			break;
		default:
			break;
	}
}
