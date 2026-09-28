import Lean.Data.Json
import Yasumaro.ModelLifecycle

namespace Yasumaro

structure ModelLifecycleCase where
  name : String
  kind : String
  mode : String
  scenario : String
  start : State
  events : List Event
  expected : State
  expectedResult : String
  brokenExpected : Option State
deriving Repr

private def unpublishedInitial : State :=
  { publishedInitial with published := false, publishedVerified := false }

private def runWith (transition : State → Event → State)
    (state : State) : List Event → State
  | [] => state
  | event :: events => runWith transition (transition state event) events

private def makeCase (name kind mode scenario expectedResult : String)
    (start : State) (events : List Event)
    (brokenExpected : Option State := none) : ModelLifecycleCase :=
  { name
    kind
    mode
    scenario
    start
    events
    expected := run start events
    expectedResult
    brokenExpected }

private def verifiedPublishEvents : List Event :=
  [.beginInstall, .createPartial, .verifyPartial, .authorizePublish, .publish]

private def unverifiedPublishEvents : List Event :=
  [.beginInstall, .createPartial, .publish, .abort true]

private def cancelBeforeAuthorizationEvents : List Event :=
  [.beginInstall, .createPartial, .verifyPartial, .requestCancel,
    .authorizePublish, .publish, .abort true]

private def cancelAfterAuthorizationEvents : List Event :=
  [.beginInstall, .createPartial, .verifyPartial, .authorizePublish,
    .requestCancel, .publish]

private def brokenUnverifiedAuthorizationStart : State :=
  { published := false
    publishedVerified := false
    «partial» := true
    partialVerified := false
    publishAuthorized := false
    cancelRequested := false
    readers := 0
    writer := true }

private def brokenAuthorizationEvents : List Event := [.authorizePublish]

private def brokenCancelAuthorizationStart : State :=
  { published := false
    publishedVerified := false
    «partial» := true
    partialVerified := true
    publishAuthorized := false
    cancelRequested := true
    readers := 0
    writer := true }

def modelLifecycleCases : List ModelLifecycleCase :=
  [makeCase "verified-publish" "install" "strict" "verified-publish"
      "success" unpublishedInitial verifiedPublishEvents,
    makeCase "unverified-publish" "install" "strict" "hash-mismatch"
      "hash-mismatch" unpublishedInitial unverifiedPublishEvents,
    makeCase "cancel-before-authorization" "install" "internal-fixture"
      "cancel-before-authorization" "cancelled" unpublishedInitial
      cancelBeforeAuthorizationEvents,
    makeCase "cancel-after-authorization" "install" "internal-fixture"
      "cancel-after-authorization" "success" unpublishedInitial
      cancelAfterAuthorizationEvents,
    makeCase "busy-remove" "remove" "strict" "busy-remove" "model-in-use"
      busyRemoveInitial [.remove],
    makeCase "remove-success" "remove" "strict" "remove-success" "success"
      stalePartialInitial [.remove],
    makeCase "broken-unverified-authorization" "sensitivity" "model-only"
      "broken-unverified-authorization" "broken-sensitivity"
      brokenUnverifiedAuthorizationStart brokenAuthorizationEvents
      (some <| runWith brokenUnverifiedAuthorizeStep
        brokenUnverifiedAuthorizationStart
        brokenAuthorizationEvents),
    makeCase "broken-cancel-authorization" "sensitivity" "model-only"
      "broken-cancel-authorization" "broken-sensitivity"
      brokenCancelAuthorizationStart brokenAuthorizationEvents
      (some <| runWith brokenCancelAuthorizeStep brokenCancelAuthorizationStart
        brokenAuthorizationEvents),
    makeCase "broken-remove-idempotency" "sensitivity" "model-only"
      "broken-remove-idempotency" "broken-sensitivity" stalePartialInitial
      [.remove, .remove]
      (some <| runWith brokenRemoveStep stalePartialInitial [.remove, .remove])]

private def renderJsonString (value : String) : String :=
  (Lean.Json.str value).compress

private def renderBool (value : Bool) : String :=
  if value then "true" else "false"

private def renderState (state : State) : String :=
  "{\"published\":" ++ renderBool state.published ++
    ",\"publishedVerified\":" ++ renderBool state.publishedVerified ++
    ",\"partial\":" ++ renderBool state.«partial» ++
    ",\"partialVerified\":" ++ renderBool state.partialVerified ++
    ",\"publishAuthorized\":" ++ renderBool state.publishAuthorized ++
    ",\"cancelRequested\":" ++ renderBool state.cancelRequested ++
    ",\"readers\":" ++ toString state.readers ++
    ",\"writer\":" ++ renderBool state.writer ++ "}"

private def renderEvent : Event → String
  | .beginInstall => renderJsonString "begin-install"
  | .createPartial => renderJsonString "create-partial"
  | .verifyPartial => renderJsonString "verify-partial"
  | .requestCancel => renderJsonString "request-cancel"
  | .authorizePublish => renderJsonString "authorize-publish"
  | .publish => renderJsonString "publish"
  | .abort true => renderJsonString "abort-cleanup-success"
  | .abort false => renderJsonString "abort-cleanup-failure"
  | .acquireLease => renderJsonString "acquire-lease"
  | .releaseLease => renderJsonString "release-lease"
  | .remove => renderJsonString "remove"

private def renderOptionalState : Option State → String
  | some state => renderState state
  | none => "null"

private def renderCase (testCase : ModelLifecycleCase) : String :=
  "{\"name\":" ++ renderJsonString testCase.name ++
    ",\"kind\":" ++ renderJsonString testCase.kind ++
    ",\"mode\":" ++ renderJsonString testCase.mode ++
    ",\"scenario\":" ++ renderJsonString testCase.scenario ++
    ",\"start\":" ++ renderState testCase.start ++
    ",\"events\":[" ++
      String.intercalate "," (testCase.events.map renderEvent) ++ "]" ++
    ",\"expected\":" ++ renderState testCase.expected ++
    ",\"expectedResult\":" ++ renderJsonString testCase.expectedResult ++
    ",\"brokenExpected\":" ++
      renderOptionalState testCase.brokenExpected ++ "}"

def modelLifecycleTestVectorsJson : String :=
  "{\"schemaVersion\":1,\"cases\":[" ++
    String.intercalate "," (modelLifecycleCases.map renderCase) ++ "]}"

end Yasumaro
