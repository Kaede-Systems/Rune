// Ghidra script: dump all function names and defined strings to a file
// @category Analysis

import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.*;
import ghidra.program.model.mem.*;
import ghidra.program.model.symbol.*;
import java.io.*;
import java.util.*;

public class ghidra_dump_functions extends GhidraScript {
    @Override
    public void run() throws Exception {
        String outputPath = System.getProperty("ghidra.dump.output", "/tmp/ghidra_dump.txt");
        PrintWriter pw = new PrintWriter(new FileWriter(outputPath));

        pw.println("=== FUNCTIONS ===");
        FunctionManager fm = currentProgram.getFunctionManager();
        FunctionIterator it = fm.getFunctions(true);
        int funcCount = 0;
        while (it.hasNext()) {
            Function f = it.next();
            pw.println(f.getEntryPoint() + "  " + f.getName() + "  " + f.getSignature());
            funcCount++;
        }
        pw.println("Total functions: " + funcCount);

        pw.println("\n=== DEFINED STRINGS ===");
        DataIterator di = currentProgram.getListing().getDefinedData(true);
        int strCount = 0;
        while (di.hasNext()) {
            Data d = di.next();
            if (d.getDataType().getName().toLowerCase().contains("string") ||
                d.getDataType().getName().equals("TerminatedCString") ||
                d.getDataType().getName().equals("string")) {
                String val = d.getDefaultValueRepresentation();
                if (val != null && val.length() > 2) {
                    pw.println(d.getAddress() + "  " + val);
                    strCount++;
                }
            }
        }
        pw.println("Total strings: " + strCount);

        pw.close();
        println("Dumped to: " + outputPath);
    }
}
