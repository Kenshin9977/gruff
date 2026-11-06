// question: `::spawnstruct` when no script function spawnstruct exists: a pointer to the builtin, or a load error?
// fact: the linker accepts a function reference to a builtin name but stores the builtin's native handler address as the script code position: calling it runs the handler's machine code as GSC bytecode (here a getdvar error on garbage, then the error recovery 'case 106')
// vs t7: a function reference falls back to the builtin and calling it works
// evidence: static: the linker's reference case (kind 1, 0x7e0926) shares the call case's lookup but does not patch the opcode: GetFunction's operand gets the handler (0x7e0a48); GetFunction (0x7edb85) pushes it as a function value and ScriptFunctionCallPointer (0x7ee91c) jumps to it as bytecode
// unsupported: no meaningful golden: the outcome is the decoding of the builtin handler's machine code as bytecode
// expect: f0 T0 before
// expect: f0 T0 script error: getdvar* expecting string or hash, given precodepos
// expect: f0 T0 called, struct=0

main()
{
	logprint("before");
	f = ::spawnstruct;
	s = [[ f ]]();
	logprint("called, struct=" + isdefined(s));
}
