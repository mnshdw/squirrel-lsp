switch (attr) {
	case 0:
		bro.getBaseProperties().Hitpoints += 1;
		icon = "ui/icons/health.png";
		text = "Hitpoint";
		inTraining.addHitpoint();
		break;

	case 1:
		bro.getBaseProperties().Bravery += 1;
		icon = "ui/icons/bravery.png";
		text = "Resolve";
		inTraining.addBrave();
		break;



	case 2:
		bro.getBaseProperties().Fatigue += 1;
		icon = "ui/icons/fatigue.png";
		text = "Fatigue";
		inTraining.addFatigue();
		break;
}
