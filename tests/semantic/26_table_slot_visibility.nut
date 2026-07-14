// EXPECT: no errors
// Table slot keys should be visible to functions in the same table

character_trait <- inherit("scripts/skills/skill", {
    m = {
        Titles = [],
        Excluded = []
    },

    function isExcluded(_id) {
        return m.Excluded.find(_id) != null;
    },

    function create() {
        m.Type = 1;
        m.Order = 2;
    }
});
