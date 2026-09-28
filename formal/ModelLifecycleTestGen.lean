import Yasumaro.ModelLifecycleTestVectors

private def generatedFixture : String :=
  Yasumaro.modelLifecycleTestVectorsJson ++ "\n"

private def usageError : IO UInt32 := do
  IO.eprintln "usage: model-lifecycle-testgen [--output <file> | --check <file>]"
  return 2

private def checkFixture (path : System.FilePath) : IO UInt32 := do
  try
    let current ← IO.FS.readFile path
    if current != generatedFixture then
      IO.eprintln "model lifecycle fixture is stale"
      return 1
    return 0
  catch _ =>
    IO.eprintln "model lifecycle fixture could not be read"
    return 1

private def writeFixture (path : System.FilePath) : IO UInt32 := do
  try
    IO.FS.writeFile path generatedFixture
    return 0
  catch _ =>
    IO.eprintln "model lifecycle fixture could not be written"
    return 1

private def run (args : List String) : IO UInt32 := do
  match args with
  | [] =>
      IO.print generatedFixture
      return 0
  | ["--output", path] => writeFixture path
  | ["--check", path] => checkFixture path
  | _ => usageError

def main (args : List String) : IO UInt32 :=
  match args with
  | "--" :: rest => run rest
  | _ => run args
