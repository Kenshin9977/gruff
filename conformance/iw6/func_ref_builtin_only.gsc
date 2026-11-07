// question: `::spawnstruct` when no script function spawnstruct exists: a pointer to the builtin, or an error?
// fact: the reference is accepted as a builtin function value (gsc-tool emits GetBuiltinFunction, which the VM has); calling it with [[ ]]() raises a script error in the script-function pointer call (opcode 0x54 takes a script function only) and the result is undefined
// evidence: the compiler is gsc-tool (IW6 ships no compiler); what IW6's own compiler emitted for `::name` of a builtin is not measured here
// vs iw5: same (only the opcode numbering differs)
// expect: f0 T0 before
// expect: f0 T0 script error: OP_ScriptFunctionCallPointer (opcode 0x54)
// expect: f0 T0 called, struct=0

main()
{
	logprint("before");
	f = ::spawnstruct;
	s = [[ f ]]();
	logprint("called, struct=" + isdefined(s));
}
