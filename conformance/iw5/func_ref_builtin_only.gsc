// question: `::spawnstruct` when no script function spawnstruct exists: a pointer to the builtin, or an error?
// fact: the reference is accepted as a builtin function value (gsc-tool emits GetBuiltinFunction, opcode 0x07, which the VM has); calling it with [[ ]]() raises a script error in the script-function pointer call (opcode 0x2f takes a script function only) and the result is undefined
// evidence: the compiler is gsc-tool (IW5 ships no compiler); what IW5's own compiler emitted for `::name` of a builtin is not measured here
// vs iw3: IW3's compiler refuses it ('unknown function')
// expect: f0 T0 before
// expect: f0 T0 script error: OP_ScriptFunctionCallPointer (opcode 0x2f)
// expect: f0 T0 called, struct=0

main()
{
	logprint("before");
	f = ::spawnstruct;
	s = [[ f ]]();
	logprint("called, struct=" + isdefined(s));
}
