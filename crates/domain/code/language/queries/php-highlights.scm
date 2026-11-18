; Before the stock query: at equal length, the first pattern wins.

; Attributes: `#[AsCommand(…)]` names a class.
(attribute (name) @type)
(attribute (qualified_name (name) @type))
(attribute_group "#[" @punctuation.bracket)
(attribute_group "]" @punctuation.bracket)

; Named arguments: `name: 'app:x'`.
(argument name: (name) @variable.parameter)
