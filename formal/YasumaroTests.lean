import Yasumaro.SpeakerAssignment
import Yasumaro.Proofs
import Yasumaro.TestVectors
import Yasumaro.ModelLifecycleProofs
import Yasumaro.ModelLifecycleTestVectors

open Yasumaro

#check step_preserves
#check run_preserves
#check verified_initial_safe
#check cleanup_success_clears_partial
#check cleanup_failure_preserves_publication
#check busy_remove_noop
#check successful_remove_clears_artifacts
#check remove_idempotent
#check unverified_partial_cannot_be_authorized
#check cancel_before_authorization_blocks_publication
#check cancel_after_authorization_preserves_authorization
#check broken_unverified_authorization_violates_safety
#check broken_cancel_authorization_is_detected
#check broken_remove_violates_idempotency

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
  assertEqual "published initial state is verified"
    publishedInitial
    { published := true, publishedVerified := true, «partial» := false,
      partialVerified := false, publishAuthorized := false,
      cancelRequested := false,
      readers := 0, writer := false }
  assertEqual "stale partial can coexist with a verified publication"
    stalePartialInitial
    { published := true, publishedVerified := true, «partial» := true,
      partialVerified := false, publishAuthorized := false,
      cancelRequested := false,
      readers := 0, writer := false }
  assertEqual "busy remove leaves the state unchanged"
    (step busyRemoveInitial .remove)
    busyRemoveInitial
  assertEqual "successful cleanup only removes the partial artifact"
    (step { stalePartialInitial with writer := true } (.abort true))
    publishedInitial
  assertEqual "successful remove clears both artifacts"
    (step stalePartialInitial .remove)
    { published := false, publishedVerified := false, «partial» := false,
      partialVerified := false, publishAuthorized := false,
      cancelRequested := false,
      readers := 0, writer := false }
  assertEqual "remove is idempotent"
    (step (step stalePartialInitial .remove) .remove)
    (step stalePartialInitial .remove)
  assertEqual "unverified partial cannot be authorized"
    (step { publishedInitial with writer := true, «partial» := true }
      .authorizePublish)
    { publishedInitial with writer := true, «partial» := true }
  let cancelledBeforeAuthorization :=
    run { publishedInitial with published := false, publishedVerified := false }
      [.beginInstall, .createPartial, .verifyPartial, .requestCancel,
        .authorizePublish, .publish]
  assertEqual "cancellation before authorization blocks publication"
    cancelledBeforeAuthorization
    { published := false, publishedVerified := false, «partial» := true,
      partialVerified := true, publishAuthorized := false,
      cancelRequested := true, readers := 0, writer := true }
  let authorizedThenCancelled :=
    run publishedInitial
      [.beginInstall, .createPartial, .verifyPartial, .authorizePublish,
        .requestCancel, .publish]
  assertEqual "cancellation after authorization does not block publication"
    authorizedThenCancelled
    { publishedInitial with cancelRequested := true }
  assertEqual "model lifecycle cases have stable names"
    (modelLifecycleCases.map (·.name))
    ["verified-publish", "unverified-publish",
      "cancel-before-authorization", "cancel-after-authorization",
      "busy-remove", "remove-success", "broken-unverified-authorization",
      "broken-cancel-authorization", "broken-remove-idempotency"]
  assertEqual "model lifecycle cases have stable modes"
    (modelLifecycleCases.map (·.mode))
    ["strict", "strict", "internal-fixture", "internal-fixture",
      "strict", "strict", "model-only", "model-only", "model-only"]
  assertEqual "model lifecycle cases have stable results"
    (modelLifecycleCases.map (·.expectedResult))
    ["success", "hash-mismatch", "cancelled", "success",
      "model-in-use", "success", "broken-sensitivity", "broken-sensitivity",
      "broken-sensitivity"]
  assertEqual "normal expectations come from the executable transition model"
    (modelLifecycleCases.map (·.expected))
    (modelLifecycleCases.map fun testCase => run testCase.start testCase.events)
  assertEqual "only sensitivity cases have broken expectations"
    (modelLifecycleCases.map (·.brokenExpected.isSome))
    [false, false, false, false, false, false, true, true, true]
  assertEqual "model lifecycle fixture is valid JSON"
    (Lean.Json.parse modelLifecycleTestVectorsJson).isOk
    true
