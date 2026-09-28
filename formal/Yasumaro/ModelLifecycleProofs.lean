import Yasumaro.ModelLifecycle

namespace Yasumaro

theorem verified_initial_safe (partialPresent : Bool) (readers : Nat) :
    Safe {
      published := true
      publishedVerified := true
      «partial» := partialPresent
      readers := readers
      writer := false
    } := by
  simp [Safe]

theorem step_preserves (state : State) (event : Event)
    (safe : Safe state) : Safe (step state event) := by
  cases event with
  | beginInstall =>
      by_cases available : state.readers = 0 ∧ state.writer = false
      · unfold Safe
        constructor
        · simpa [step, available] using safe.1
        · simp [step, available]
      · simpa [step, available] using safe
  | createPartial =>
      by_cases writer : state.writer = true
      · simpa [step, writer, Safe] using safe
      · simpa [step, writer] using safe
  | publishVerified =>
      by_cases ready : state.writer = true ∧ state.«partial» = true
      · simp [step, ready, Safe]
      · simpa [step, ready] using safe
  | abort cleanupSucceeded =>
      by_cases writer : state.writer = true
      · unfold Safe
        constructor
        · simpa [step, writer] using safe.1
        · simp [step, writer]
      · simpa [step, writer] using safe
  | acquireLease =>
      by_cases available : state.writer = false ∧ state.published = true ∧
          state.publishedVerified = true
      · unfold Safe
        constructor
        · simp [step, available]
        · simp [step, available]
      · simpa [step, available] using safe
  | releaseLease =>
      by_cases hasReader : 0 < state.readers
      · simp only [step, hasReader]
        unfold Safe
        constructor
        · exact safe.1
        · intro writer
          have noReaders := safe.2 writer
          omega
      · simpa [step, hasReader] using safe
  | remove =>
      by_cases available : state.writer = false ∧ state.readers = 0
      · simp [step, available, Safe]
      · simpa [step, available] using safe

theorem run_preserves (state : State) (events : List Event)
    (safe : Safe state) : Safe (run state events) := by
  induction events generalizing state with
  | nil => simpa [run]
  | cons event events ih =>
      simp only [run]
      exact ih (step state event) (step_preserves state event safe)

theorem cleanup_success_clears_partial (state : State)
    (writer : state.writer = true) :
    (step state (.abort true)).«partial» = false := by
  simp [step, writer]

theorem cleanup_failure_preserves_publication (state : State) :
    (step state (.abort false)).published = state.published ∧
      (step state (.abort false)).publishedVerified = state.publishedVerified := by
  by_cases writer : state.writer = true <;> simp [step, writer]

theorem busy_remove_noop (state : State)
    (busy : state.writer = true ∨ 0 < state.readers) :
    step state .remove = state := by
  by_cases available : state.writer = false ∧ state.readers = 0
  · rcases busy with writer | readers
    · simp_all
    · omega
  · simp [step, available]

theorem successful_remove_clears_artifacts (state : State)
    (writer : state.writer = false) (readers : state.readers = 0) :
    (step state .remove).published = false ∧
      (step state .remove).«partial» = false := by
  simp [step, writer, readers]

theorem remove_idempotent (state : State) :
    step (step state .remove) .remove = step state .remove := by
  by_cases available : state.writer = false ∧ state.readers = 0 <;>
    simp [step, available]

end Yasumaro
