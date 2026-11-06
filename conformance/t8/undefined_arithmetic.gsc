// question: what does undefined + 1 do at run time: an error that ends the thread, or a value?
// fact: undefined + 1 is a script error (code #12e690eb); the error reporter ends in Com_Error(0x20), which in the dump's session state aborts the run; when Com_Error returns, the thread goes on and the result is undefined
// vs t7: T7 (retail): a non-terminal script error and the thread goes on
// evidence: static: ScrVm_Error (0x276e530) calls the reporter (0x27758b0), which always calls Com_Error (0x2889710) with code 0x20; Com_Error returns for 0x20 only when the byte 0x8a57023 is set (not identified), else it aborts to the main loop; after a return ScrVm_Error longjmps into VM_Execute's recovery
// expect: f0 T0 before
// expect: f0 T0 script error: #12e690eb
// expect error: Com_Error 0x20
#namespace zz_test;

function main()
{
	iprintln("before");
	x = undefined;
	y = x + 1;
	iprintln("after " + isdefined(y));
}
