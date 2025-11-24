// EXPECT: 0 errors
// Nested tables inside array (common tooltip pattern)
function getTooltip() {
    local ret = [
        {
            id = 1,
            type = "title",
            text = "My Title"
        },
        {
            id = 2,
            type = "description",
            text = "Some description"
        }
    ];
    return ret;
}
