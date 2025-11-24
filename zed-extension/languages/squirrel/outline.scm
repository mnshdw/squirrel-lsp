(class_declaration
  (identifier) @name) @item

(function_declaration
  (identifier) @name) @item

(member_declaration
  (function_declaration
    (identifier) @name)) @item

(member_declaration
  "constructor" @name) @item

(enum_declaration
  (identifier) @name) @item

(const_declaration
  (identifier) @name) @item
