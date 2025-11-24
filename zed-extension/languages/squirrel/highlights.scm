; Keywords
[
  "class"
  "extends"
  "enum"
  "var"
  "local"
  "static"
  "const"
  "if"
  "else"
  "switch"
  "case"
  "default"
  "break"
  "for"
  "foreach"
  "in"
  "do"
  "while"
  "continue"
  "try"
  "catch"
  "throw"
  "function"
  "return"
  "yield"
  "typeof"
  "instanceof"
  "rawcall"
] @keyword

; clone, delete, resume are aliased in grammar - match via parent nodes
(clone_expression) @keyword
(delete_expression) @keyword
(resume_expression) @keyword

; Variables
(identifier) @variable
(global_variable) @variable

; Parameters
(parameter (identifier) @variable.parameter)

; Properties
(deref_expression "." (identifier) @property)
(table_slot (identifier) @property)

; Types
(class_declaration (identifier) @type)
(enum_declaration (identifier) @type)

; Functions
(function_declaration (identifier) @function)
(call_expression function: (identifier) @function)
(member_declaration "constructor" @constructor)

; Constants
(const_declaration (identifier) @constant)

; Operators
[
  "+" "-" "*" "/" "%"
  "||" "&&" "|" "^" "&"
  "==" "!=" "<=>" ">" ">=" "<=" "<"
  "<<" ">>" ">>>"
  "=" "<-" "+=" "-=" "*=" "/=" "%="
  "~" "!" "++" "--"
] @operator

; Punctuation
["{" "}" "[" "]" "(" ")" "</" "/>"] @punctuation.bracket
["." "," ";" ":"] @punctuation.delimiter
["::" "..."] @punctuation.special

; Literals
(string) @string
(verbatim_string) @string
(char) @string
(escape_sequence) @string.escape
(integer) @number
(float) @number
(bool) @boolean
(null) @constant.builtin

; Comments
(comment) @comment
