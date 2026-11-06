// question: a non-array value passed to a &name parameter
// unsupported: no golden: stop mode ends at the first error (Com_Error 0x20); continue mode repeats the error 61 times, then #fd7ba5b1 and the terminal #38685e3b (VM_Execute's invalid-opcode code in its recovery loop), the T7 pattern; the outcome depends on the bytes after the operand
// fact: a non-array given to a &name parameter is a script error (code #f5ef3c53, T7's 'Var ByRef only supported for array types'); with script errors continuing, the VM's recovery repeats it, as in T7
// vs t7: same error position; T7's recovery resumes inside SafeCreateLocalVariables' operands (semantics/t7.toml)
#namespace zz_test;

function change(&v)
{
}

function main()
{
	x = 5;
	change(x);
	iprintln("x=" + x);
}
