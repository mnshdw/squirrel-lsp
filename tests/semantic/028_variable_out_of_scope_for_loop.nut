// EXPECT: no errors
// Variables defined outside of a for loop should be accessible inside the loop.

local randomVillage;
for (local i = 0; i != this.World.EntityManager.getSettlements().len(); ++i) {
  randomVillage = this.World.EntityManager.getSettlements()[i];
}
