--------------------------- MODULE LinuxTray ---------------------------
EXTENDS Naturals, Sequences, Probe
CONSTANT MaxPending, RecoveryOnly
VARIABLES surface, queue, mailbox, worker, sampled, grace, active, wake,
          badHide, badSettings, badResize
vars == <<surface, queue, mailbox, worker, sampled, grace, active, wake,
          badHide, badSettings, badResize>>
Range(q) == {q[i] : i \in DOMAIN q}

\* 0 Waiting, 1 Available, 2 Unavailable. Probe executes real Rust operations.
Core(s, c, d, t) == [state |-> s, content |-> c, diagnostic |-> d,
                     created |-> t, exited |-> FALSE]
Sample == {"observe00", "observe01", "observe10", "observe11", "observe01_fail", "observe11_fail"}
Intents == {"activate", "show", "settings", "hide", "resize", "retry_ok", "retry_fail", "close", "quit"}
Init == /\ surface \in {Core(0, FALSE, FALSE, TRUE), Core(2, FALSE, TRUE, FALSE)}
        /\ queue = <<>> /\ mailbox = "none" /\ worker = "idle"
        /\ sampled = "observe00" /\ grace = FALSE /\ active = "none" /\ wake = FALSE
        /\ badHide = FALSE /\ badSettings = FALSE /\ badResize = FALSE

\* Finite timer abstraction. It expires once; samples carry completion-time grace.
ExpireGrace == /\ ~surface.exited /\ ~grace /\ grace' = TRUE
               /\ UNCHANGED <<surface, queue, mailbox, worker, sampled, active, wake,
                               badHide, badSettings, badResize>>
ReadHost == /\ ~surface.exited /\ worker = "idle"
            /\ sampled' \in IF RecoveryOnly THEN {IF surface.created THEN (IF grace THEN "observe11" ELSE "observe10")
                                                ELSE (IF grace THEN "observe01" ELSE "observe00")}
                           ELSE IF grace THEN {"observe01", "observe11", "observe01_fail", "observe11_fail"} ELSE {"observe00", "observe10"}
            /\ worker' = "read" /\ wake' = FALSE
            /\ UNCHANGED <<surface, queue, mailbox, grace, active, badHide, badSettings, badResize>>

\* publish() holds the mailbox mutex through overwrite and dispatch. One drain
\* may be queued. New samples overwrite latest while scheduled remains true.
Publish == /\ ~surface.exited /\ worker = "read"
           /\ ("drain" \in Range(queue) \/ Len(queue) < MaxPending)
           /\ mailbox' = sampled
           /\ queue' = IF "drain" \in Range(queue) THEN queue ELSE Append(queue, "drain")
           /\ worker' = "idle"
           /\ UNCHANGED <<surface, sampled, grace, active, wake, badHide, badSettings, badResize>>

\* One GTK callback at a time. Mailbox lock is released before observe_on_main;
\* the worker may publish another sample between Dequeue and Apply.
Dequeue == /\ ~surface.exited /\ active = "none" /\ Len(queue) > 0
           /\ active' = IF Head(queue) = "drain" THEN mailbox ELSE Head(queue)
           /\ mailbox' = IF Head(queue) = "drain" THEN "none" ELSE mailbox
           /\ queue' = Tail(queue)
           /\ UNCHANGED <<surface, worker, sampled, grace, wake, badHide, badSettings, badResize>>

\* State reads, GTK effects, Settings emission, programmatic diagnostic hide,
\* and delete callbacks cannot interleave with another GTK callback. Workers
\* never write these values. This table is measured from the actual Rust code.
Apply == /\ ~surface.exited /\ active # "none"
         /\ LET result == SourceStep(surface, active) IN
            /\ surface' = [state |-> result.state, content |-> result.content,
                            diagnostic |-> result.diagnostic, created |-> result.created,
                            exited |-> result.exited]
            /\ badHide' = (badHide \/ (active = "hide" /\ surface.state = 2 /\
                               (result.content \/ ~result.diagnostic) /\ ~result.exited))
            /\ badSettings' = (badSettings \/ (result.settings /\ result.state # 1))
            /\ badResize' = (badResize \/ (active = "resize" /\ surface.state # 1 /\
                       (result.apply \/ result.content # surface.content \/ result.diagnostic # surface.diagnostic)))
         /\ wake' = IF active \in {"retry_ok", "retry_fail"} THEN TRUE ELSE wake
         /\ active' = "none"
         /\ UNCHANGED <<queue, mailbox, worker, sampled, grace>>

Enqueue == /\ ~surface.exited /\ Len(queue) < MaxPending /\ ~RecoveryOnly
           /\ \E intent \in Intents:
               /\ (intent # "close" \/ surface.diagnostic)
               /\ queue' = Append(queue, intent)
           /\ UNCHANGED <<surface, mailbox, worker, sampled, grace, active, wake,
                           badHide, badSettings, badResize>>
\* Recovery requires no user Retry: after grace, the constructor succeeds and
\* sustained own-registration samples arrive. Failed attempts are arbitrary in
\* safety; this recovery suffix assumes the external host now permits creation.
Next == ExpireGrace \/ ReadHost \/ Publish \/ Dequeue \/ Apply \/ Enqueue
TypeOK == /\ surface.state \in 0..2
          /\ surface.content \in BOOLEAN /\ surface.diagnostic \in BOOLEAN
          /\ surface.created \in BOOLEAN /\ surface.exited \in BOOLEAN
          /\ queue \in UNION {[1..n -> Intents \cup {"drain"}] : n \in 0..MaxPending}
          /\ mailbox \in Sample \cup {"none"} /\ worker \in {"idle", "read"}
          /\ sampled \in Sample /\ grace \in BOOLEAN /\ active \in Sample \cup Intents \cup {"none"}
          /\ wake \in BOOLEAN /\ badHide \in BOOLEAN /\ badSettings \in BOOLEAN /\ badResize \in BOOLEAN
TrayRequiredForContent == surface.exited \/ ~surface.content \/ surface.state = 1
OnlyDiagnosticWithoutTray == surface.exited \/ surface.state # 2 \/ (~surface.content /\ surface.diagnostic)
HideRequiresObservedTray == ~badHide
SettingsRequiresObservedTray == ~badSettings
ResizePreservesUnavailableSurface == ~badResize
EventuallyRecovers == <> (surface.state = 1 /\ surface.content /\ ~surface.diagnostic)
Spec == Init /\ [][Next]_vars
RecoverySpec == Spec /\ WF_vars(ReadHost) /\ WF_vars(Publish) /\ WF_vars(Dequeue)
                /\ WF_vars(Apply) /\ WF_vars(ExpireGrace)
=============================================================================
