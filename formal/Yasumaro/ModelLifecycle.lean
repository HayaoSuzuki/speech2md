import Std.Tactic

namespace Yasumaro

structure State where
  published : Bool
  publishedVerified : Bool
  «partial» : Bool
  readers : Nat
  writer : Bool
deriving BEq, DecidableEq, Repr

inductive Event
  | beginInstall
  | createPartial
  | publishVerified
  | abort (cleanupSucceeded : Bool)
  | acquireLease
  | releaseLease
  | remove
deriving BEq, DecidableEq, Repr

def Safe (state : State) : Prop :=
  (state.published = true → state.publishedVerified = true) ∧
    (state.writer = true → state.readers = 0)

def step (state : State) : Event → State
  | .beginInstall =>
      if state.readers = 0 ∧ state.writer = false then
        { state with writer := true }
      else
        state
  | .createPartial =>
      if state.writer = true then
        { state with «partial» := true }
      else
        state
  | .publishVerified =>
      if state.writer = true ∧ state.«partial» = true then
        { state with
            published := true
            publishedVerified := true
            «partial» := false
            writer := false }
      else
        state
  | .abort cleanupSucceeded =>
      if state.writer = true then
        { state with
            «partial» := if cleanupSucceeded then false else state.«partial»
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
            «partial» := false }
      else
        state

def run (state : State) : List Event → State
  | [] => state
  | event :: events => run (step state event) events

def publishedInitial : State :=
  { published := true
    publishedVerified := true
    «partial» := false
    readers := 0
    writer := false }

def stalePartialInitial : State :=
  { publishedInitial with «partial» := true }

def busyRemoveInitial : State :=
  { stalePartialInitial with readers := 1 }

end Yasumaro
