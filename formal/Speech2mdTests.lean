import Speech2md.SpeakerAssignment
import Speech2md.Proofs
import Speech2md.TestVectors

open Speech2md

private def tenPercent : OverlapThreshold := ⟨1, 10, by omega, by omega⟩
private def quarter : OverlapThreshold := ⟨1, 4, by omega, by omega⟩

private def assertEqual [BEq α] [Repr α]
    (name : String) (actual expected : α) : IO Unit :=
  if actual == expected then
    pure ()
  else
    throw <| IO.userError s!"{name}: expected {repr expected}, got {repr actual}"

def main : IO Unit := do
  assertEqual "an overlap ratio above one is rejected"
    (OverlapThreshold.create 2 1) none
  assertEqual "a reversed span is rejected"
    (TimeSpan.create 10 0) none
  assertEqual "overlap is zero for disjoint spans"
    (overlapMs ⟨0, 10, by omega⟩ ⟨10, 20, by omega⟩) 0
  assertEqual "greatest overlap selects the speaker"
    (assignSpeaker ⟨0, 10, by omega⟩
      [⟨0, ⟨0, 4, by omega⟩⟩, ⟨1, ⟨3, 10, by omega⟩⟩] tenPercent)
    (some 1)
  assertEqual "equal overlap selects the smaller speaker id regardless of order"
    (assignSpeaker ⟨0, 10, by omega⟩
      [⟨1, ⟨0, 5, by omega⟩⟩, ⟨0, ⟨5, 10, by omega⟩⟩] tenPercent)
    (some 0)
  assertEqual "overlap below threshold is unknown"
    (assignSpeaker ⟨0, 10, by omega⟩ [⟨0, ⟨0, 2, by omega⟩⟩] quarter)
    none
  assertEqual "test vectors are generated from the executable model"
    testVectorsJson
    "[{\"name\":\"greatest-overlap\",\"utterance\":{\"startMs\":0,\"endMs\":10},\"segments\":[{\"speaker\":0,\"startMs\":0,\"endMs\":4},{\"speaker\":1,\"startMs\":3,\"endMs\":10}],\"threshold\":{\"numerator\":1,\"denominator\":10},\"expectedSpeaker\":1},{\"name\":\"tie-smaller-id\",\"utterance\":{\"startMs\":0,\"endMs\":10},\"segments\":[{\"speaker\":1,\"startMs\":0,\"endMs\":5},{\"speaker\":0,\"startMs\":5,\"endMs\":10}],\"threshold\":{\"numerator\":1,\"denominator\":10},\"expectedSpeaker\":0},{\"name\":\"below-threshold\",\"utterance\":{\"startMs\":0,\"endMs\":10},\"segments\":[{\"speaker\":0,\"startMs\":0,\"endMs\":2}],\"threshold\":{\"numerator\":1,\"denominator\":4},\"expectedSpeaker\":null}]"
