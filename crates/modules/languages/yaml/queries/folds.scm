; The key stays visible, and the label counts the value's keys / items.
(block_mapping_pair value: (block_node (block_mapping) @count.object)) @fold
(block_mapping_pair value: (block_node (block_sequence) @count.array)) @fold
(block_sequence_item (block_node (block_mapping) @count.object)) @fold
(flow_mapping) @fold.object
(flow_sequence) @fold.array
(block_mapping_pair value: (block_node (block_scalar))) @fold
