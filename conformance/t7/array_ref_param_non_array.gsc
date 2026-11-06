// question: a non-array value passed to a &name parameter
// fact: a &name parameter given a non-array is a script error 'Var ByRef only supported for array types', raised by the callee's SafeCreateLocalVariables; the VM's error recovery then resumes inside that opcode's operands, so the next 'opcode' is its count byte and a padding byte (here 0x6a01, an invalid opcode): developer 1 ends in Com_Error 'server script error', retail re-throws the same invalid opcode forever (the harness stops it after 64 errors). The caller's next statement never runs.
// expect: f0 T0 script error: Var ByRef only supported for array types
// evidence: trace (t7_run.py --trace --ops): SafeCreateLocalVariables at change+0 throws the ByRef error; the recovery re-dispatch runs that opcode's error variant; the next dispatch reads offset +2 (count 0x01, padding 0x6a) as opcode 0x6a01 and throws 'Invalid opcode' from the same position each time
// evidence: static: VM_Execute (0x12edb30) saves pos before each opcode and, after the longjmp, restores it and calls the recovery 0x12ee0f0, which re-dispatches the opcode through the error tables (0x3277350/0x3297350); the error variant of SafeCreateLocalVariables (0x12ce880) is a bare ret, so its operands (count, padding, u32 name hashes, flags) are not skipped, unlike e.g. ScriptFunctionCall's (0x12cee40). An invalid opcode in the recovery throws again (0x12ee1a2); retail Scr_Throw (0x12ea4e0) longjmps without a terminal check and the recovery clears the terminal flag before RuntimeErrorInternal (0x12d4e40), so nothing ends the loop
// unsupported: no golden trace: the game's own error recovery never ends in retail (a harness limit stops it) and ends in Com_Error in developer mode; the outcome also depends on the compiler's padding byte (a valid opcode value would run instead)
#namespace zz_test;

function change(&v)
{
}

function main()
{
	x = 5;
	change(x);
	logprint("x=" + x);
}
