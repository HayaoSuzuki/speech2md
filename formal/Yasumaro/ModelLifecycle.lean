import Std.Tactic

namespace Yasumaro

structure State where
  published : Bool
  publishedVerified : Bool
  «partial» : Bool
  partialVerified : Bool
  publishAuthorized : Bool
  cancelRequested : Bool
  readers : Nat
  writer : Bool
deriving BEq, DecidableEq, Repr

inductive Event
  | beginInstall
  | createPartial
  | verifyPartial
  | requestCancel
  | authorizePublish
  | publish
  | abort (cleanupSucceeded : Bool)
  | acquireLease
  | releaseLease
  | remove
deriving BEq, DecidableEq, Repr

def Safe (state : State) : Prop :=
  (state.published = true → state.publishedVerified = true) ∧
    (state.partialVerified = true → state.«partial» = true) ∧
    (state.publishAuthorized = true →
      state.partialVerified = true ∧ state.writer = true) ∧
    (state.writer = true → state.readers = 0)

def step (state : State) : Event → State
  | .beginInstall =>
      if state.readers = 0 ∧ state.writer = false ∧
          state.cancelRequested = false then
        { state with writer := true, publishAuthorized := false }
      else
        state
  | .createPartial =>
      if state.writer = true then
        { state with
            «partial» := true
            partialVerified := false
            publishAuthorized := false }
      else
        state
  | .verifyPartial =>
      if state.writer = true ∧ state.«partial» = true then
        { state with partialVerified := true }
      else
        state
  | .requestCancel =>
      { state with cancelRequested := true }
  | .authorizePublish =>
      if state.writer = true ∧ state.«partial» = true ∧
          state.partialVerified = true ∧ state.cancelRequested = false then
        { state with publishAuthorized := true }
      else
        state
  | .publish =>
      if state.writer = true ∧ state.«partial» = true ∧
          state.partialVerified = true ∧ state.publishAuthorized = true then
        { state with
            published := true
            publishedVerified := true
            «partial» := false
            partialVerified := false
            publishAuthorized := false
            writer := false }
      else
        state
  | .abort cleanupSucceeded =>
      if state.writer = true then
        { state with
            «partial» := if cleanupSucceeded then false else state.«partial»
            partialVerified :=
              if cleanupSucceeded then false else state.partialVerified
            publishAuthorized := false
            writer := false }
      else
        state
  | .acquireLease =>
      if state.writer = false ∧ state.published = true ∧
          state.publishedVerified = true then
        { state with readers := state.readers + 1 }
      else
        state
  | .releaseLease =>
      if 0 < state.readers then
        { state with readers := state.readers - 1 }
      else
        state
  | .remove =>
      if state.writer = false ∧ state.readers = 0 then
        { state with
            published := false
            publishedVerified := false
            «partial» := false
            partialVerified := false
            publishAuthorized := false }
      else
        state

def brokenAuthorizeStep (state : State) : Event → State
  | .authorizePublish =>
      if state.writer = true ∧ state.«partial» = true then
        { state with publishAuthorized := true }
      else
        state
  | event => step state event

def run (state : State) : List Event → State
  | [] => state
  | event :: events => run (step state event) events

def publishedInitial : State :=
  { published := true
    publishedVerified := true
    «partial» := false
    partialVerified := false
    publishAuthorized := false
    cancelRequested := false
    readers := 0
    writer := false }

def stalePartialInitial : State :=
  { publishedInitial with «partial» := true }

def busyRemoveInitial : State :=
  { stalePartialInitial with readers := 1 }

end Yasumaro
