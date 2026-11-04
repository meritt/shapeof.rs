import ShapeOf

let inferred = try ShapeOf.schema("{\"id\": 1}", for: .openai)
precondition(inferred.notes.rootObject)
