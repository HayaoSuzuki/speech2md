import Lean.Data.Json
import Speech2md.SpeakerAssignment

namespace Speech2md

private def renderJsonString (value : String) : String :=
  (Lean.Json.str value).compress

private def renderExpectedSpeaker (value : Option Nat) : String :=
  match value with
  | some speaker => toString speaker
  | none => "null"

private def renderSpan (span : TimeSpan) : String :=
  "{\"startMs\":" ++ toString span.startMs ++
    ",\"endMs\":" ++ toString span.endMs ++ "}"

private def renderSegment (segment : SpeakerSegment) : String :=
  "{\"speaker\":" ++ toString segment.speaker ++
    ",\"startMs\":" ++ toString segment.span.startMs ++
    ",\"endMs\":" ++ toString segment.span.endMs ++ "}"

private def renderThreshold (threshold : OverlapThreshold) : String :=
  "{\"numerator\":" ++ toString threshold.numerator ++
    ",\"denominator\":" ++ toString threshold.denominator ++ "}"

private def assignmentCaseJson (name : String) (utterance : TimeSpan)
    (segments : List SpeakerSegment) (threshold : OverlapThreshold) : String :=
  let expected := assignSpeaker utterance segments threshold
  "{\"name\":" ++ renderJsonString name ++
    ",\"utterance\":" ++ renderSpan utterance ++
    ",\"segments\":[" ++
      String.intercalate "," (segments.map renderSegment) ++ "]" ++
    ",\"threshold\":" ++ renderThreshold threshold ++
    ",\"expectedSpeaker\":" ++ renderExpectedSpeaker expected ++ "}"

def testVectorsJson : String :=
  let utterance : TimeSpan := ⟨0, 10, by omega⟩
  let tenPercent : OverlapThreshold := ⟨1, 10, by omega, by omega⟩
  let quarter : OverlapThreshold := ⟨1, 4, by omega, by omega⟩
  let greatest := assignmentCaseJson "greatest-overlap" utterance
    [⟨0, ⟨0, 4, by omega⟩⟩, ⟨1, ⟨3, 10, by omega⟩⟩] tenPercent
  let tie := assignmentCaseJson "tie-smaller-id" utterance
    [⟨1, ⟨0, 5, by omega⟩⟩, ⟨0, ⟨5, 10, by omega⟩⟩] tenPercent
  let below := assignmentCaseJson "below-threshold" utterance
    [⟨0, ⟨0, 2, by omega⟩⟩] quarter
  "[" ++ String.intercalate "," [greatest, tie, below] ++ "]"

end Speech2md
