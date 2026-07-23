; Derived from lucascompython/wgsl-wesl-zed (MIT).

(function_decl
  body: (_) @function.inside) @function.around

(struct_decl
  body: (_) @class.inside) @class.around

[
  (line_comment)
  (block_comment)
] @comment.inside

(line_comment)+ @comment.around

(block_comment) @comment.around
