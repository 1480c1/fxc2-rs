/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::{
    collections::VecDeque,
    env,
    ffi::{c_void, CStr, CString},
    fmt,
    fs::File,
    io::{Read, Write},
    process::ExitCode,
    slice,
};

use windows::{
    core::PCSTR,
    Win32::Graphics::{
        Direct3D::{
            Fxc::{
                D3DCompile2, D3DCOMPILE_ALL_RESOURCES_BOUND, D3DCOMPILE_AVOID_FLOW_CONTROL,
                D3DCOMPILE_DEBUG, D3DCOMPILE_ENABLE_BACKWARDS_COMPATIBILITY,
                D3DCOMPILE_ENABLE_STRICTNESS, D3DCOMPILE_ENABLE_UNBOUNDED_DESCRIPTOR_TABLES,
                D3DCOMPILE_IEEE_STRICTNESS, D3DCOMPILE_NO_PRESHADER,
                D3DCOMPILE_OPTIMIZATION_LEVEL0, D3DCOMPILE_OPTIMIZATION_LEVEL1,
                D3DCOMPILE_OPTIMIZATION_LEVEL3, D3DCOMPILE_PACK_MATRIX_COLUMN_MAJOR,
                D3DCOMPILE_PACK_MATRIX_ROW_MAJOR, D3DCOMPILE_PARTIAL_PRECISION,
                D3DCOMPILE_RESOURCES_MAY_ALIAS, D3DCOMPILE_SKIP_OPTIMIZATION,
                D3DCOMPILE_SKIP_VALIDATION, D3DCOMPILE_WARNINGS_ARE_ERRORS,
            },
            ID3DBlob, ID3DInclude, D3D_SHADER_MACRO,
        },
        Hlsl::{D3DCOMPILE_OPTIMIZATION_LEVEL2, D3D_COMPILE_STANDARD_FILE_INCLUDE},
    },
};

struct ProfilePrefix {
    name: &'static str,
    prefix: &'static str,
}

static PROFILE_PREFIX_TABLE: [ProfilePrefix; 12] = [
    ProfilePrefix {
        name: "ps_2_0",
        prefix: "g_ps20",
    },
    ProfilePrefix {
        name: "ps_2_a",
        prefix: "g_ps21",
    },
    ProfilePrefix {
        name: "ps_2_b",
        prefix: "g_ps21",
    },
    ProfilePrefix {
        name: "ps_2_sw",
        prefix: "g_ps2ff",
    },
    ProfilePrefix {
        name: "ps_3_0",
        prefix: "g_ps30",
    },
    ProfilePrefix {
        name: "ps_3_sw",
        prefix: "g_ps3ff",
    },
    ProfilePrefix {
        name: "vs_1_1",
        prefix: "g_vs11",
    },
    ProfilePrefix {
        name: "vs_2_0",
        prefix: "g_vs20",
    },
    ProfilePrefix {
        name: "vs_2_a",
        prefix: "g_vs21",
    },
    ProfilePrefix {
        name: "vs_2_sw",
        prefix: "g_vs2ff",
    },
    ProfilePrefix {
        name: "vs_3_0",
        prefix: "g_vs30",
    },
    ProfilePrefix {
        name: "vs_3_sw",
        prefix: "g_vs3ff",
    },
];

enum UsageError {
    HelpRequested,
    UnknownArgument(String),
    MissingArgument(String),
    TooManyArguments,
}

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UsageError::HelpRequested => write!(f, "Check https://learn.microsoft.com/en-us/windows/win32/direct3dtools/dx-graphics-tools-fxc-syntax for usage information."),
            UsageError::UnknownArgument(arg) => {
                writeln!(f, "Unknown argument: '{arg}'")?;
                writeln!(f, "This isn't a sign of disaster, odds are it will be very easy to add support for this argument.")?;
                writeln!(f, "Review the meaning of the argument in the real fxc program, and then add it into fxc2.")
            }
            UsageError::MissingArgument(arg) => {
                writeln!(f, "Missing argument for: '{arg}'")?;
                writeln!(f, "We expected to receive this, and it's likely things will nmot work correctly without it.")?;
                writeln!(f, "Review fxc2 and make sure things will work.")
            }
            UsageError::TooManyArguments => write!(f, "You specified multiple input files. We did not expect to receive this, and aren't prepared to handle multiple input files. You'll have to edit the source to behave the way you want."),
        }
    }
}

impl From<UsageError> for ExitCode {
    fn from(err: UsageError) -> ExitCode {
        eprintln!("{err}");
        ExitCode::FAILURE
    }
}

enum Opts {
    /// (T), Required
    Model(String),
    /// (?, help), Optional
    Help,
    /// (all_resources_bound), Optional
    AllResourcesBound,
    /// (D), Optional
    Define(CString, CString),
    /// (E), Required
    EntryPointName(CString),
    /// (enable_unbounded_descriptor_tables), Optional
    UnboundedDescriptorTables,
    /// (Fh), Required
    OutputFile(String),
    /// (Gec), Optional
    BackwardsCompatibility,
    /// (Ges), Optional
    EnableStrictness,
    /// (Gfa), Optional
    AvoidFlowControl,
    /// (Gis), Optional
    EnableIEEEStrictness,
    /// (Gpp), Optional
    PartialPrecision,

    // Don't know how to handle includes yet
    /// (nologo), Optional
    NoLogo,
    /// (Od), Optional
    DisableOptimizations,
    /// (Op), Optional
    DisablePreshaders,
    /// (O0), Optional
    OptimizationLevel0,
    /// (O1), Optional
    OptimizationLevel1,
    /// (O2), Optional
    OptimizationLevel2,
    /// (O3), Optional
    OptimizationLevel3,
    /// (res_may_alias), Optional
    ResourceMayAlias,
    /// (Vd), Optional
    SkipValidation,
    /// (Vi), Optional
    OutputIncludeProcessDetails,
    /// (Vn), Optional
    VariableName(String),
    /// (WX), Optional
    WarningsAsErrors,
    /// (Zi), Optional
    DebugInformation,
    /// (Zpc), Optional
    PackMatrixColumnMajor,
    /// (Zpr)), Optional
    PackMatrixRowMajor,
    /// (), Input file
    InputFile(String),
}

impl Opts {
    /// Parses the first argument. If the argument requires an argument, and it is not already attached to the first, the next argument is used.
    /// Returns true if the second argument was used.
    fn parse(first: &str, args: &mut VecDeque<String>) -> Result<Opts, UsageError> {
        let first_char = first.chars().next().unwrap();
        match first.len() {
            0 => panic!("Empty argument"),
            1 | _ if first_char != '-' && first_char != '/' => {
                // not an option, assume it's the input file
                return Ok(Opts::InputFile(first.to_owned()));
            }
            _ => {}
        }
        // trim the '-' or '/'
        let mut first = &first[1..];
        // handle no-arg options
        match first {
            "?" | "help" => return Ok(Opts::Help),
            "all_resources_bound" => return Ok(Opts::AllResourcesBound),
            "enable_unbounded_descriptor_tables" => return Ok(Opts::UnboundedDescriptorTables),
            "Gec" => return Ok(Opts::BackwardsCompatibility),
            "Ges" => return Ok(Opts::EnableStrictness),
            "Gfa" => return Ok(Opts::AvoidFlowControl),
            "Gis" => return Ok(Opts::EnableIEEEStrictness),
            "Gpp" => return Ok(Opts::PartialPrecision),
            "nologo" => return Ok(Opts::NoLogo),
            "Od" => return Ok(Opts::DisableOptimizations),
            "Op" => return Ok(Opts::DisablePreshaders),
            "O0" => return Ok(Opts::OptimizationLevel0),
            "O1" => return Ok(Opts::OptimizationLevel1),
            "O2" => return Ok(Opts::OptimizationLevel2),
            "O3" => return Ok(Opts::OptimizationLevel3),
            "res_may_alias" => return Ok(Opts::ResourceMayAlias),
            "Vd" => return Ok(Opts::SkipValidation),
            "Vi" => return Ok(Opts::OutputIncludeProcessDetails),
            "WX" => return Ok(Opts::WarningsAsErrors),
            "Zi" => return Ok(Opts::DebugInformation),
            "Zpc" => return Ok(Opts::PackMatrixColumnMajor),
            "Zpr" => return Ok(Opts::PackMatrixRowMajor),
            _ => {}
        }
        // handle options with arguments.
        // First check if the argument is attached to the option
        let mut argument: String = String::new();
        const ARG_PREFIX: [&str; 5] = ["T", "D", "E", "Fh", "Vn"];
        for prefix in ARG_PREFIX.iter() {
            if !first.starts_with(prefix) {
                continue;
            }
            first = prefix;
            let arg = &first[prefix.len()..];
            if !arg.is_empty() {
                argument = arg.to_owned();
                break;
            }
            if let Some(second) = args.pop_front() {
                argument = second;
                break;
            }
            return Err(UsageError::MissingArgument(first.to_owned()));
        }
        match first {
            "T" => Ok(Opts::Model(argument)),
            "D" => {
                let mut define = argument.split('=');
                let name =
                    CString::new(define.next().unwrap()).expect("Failed to parse define name");
                let value = CString::new(define.next().unwrap_or("1"))
                    .expect("Failed to parse define value");
                Ok(Opts::Define(name, value))
            }
            "E" => Ok(Opts::EntryPointName(
                CString::new(argument).expect("Failed to parse entry point name"),
            )),
            "Fh" => Ok(Opts::OutputFile(argument)),
            "Vn" => Ok(Opts::VariableName(argument)),
            _ => Err(UsageError::UnknownArgument(first.to_owned())),
        }
    }
}

struct CompileOutput {
    data: Option<ID3DBlob>,
    errors: Option<ID3DBlob>,
}

impl Default for CompileOutput {
    fn default() -> Self {
        Self {
            data: None,
            errors: None,
        }
    }
}

struct ParseOpt {
    model: String,
    entry_point: CString,
    variable_name: String,
    output_file: String,
    // defines: Vec<(CString, CString)>,
    d3d_defines: Vec<D3D_SHADER_MACRO>,
    input_file: String,
    flags1: u32,
}

impl ParseOpt {
    fn new() -> Result<ParseOpt, UsageError> {
        let mut args = env::args().skip(1).collect::<VecDeque<String>>();

        let mut n_model = String::new();
        let mut n_entry_point = CString::new("").unwrap();
        let mut n_variable_name = String::new();
        let mut n_output_file = String::new();
        let mut n_defines = Vec::new();
        let mut n_input_file = String::new();
        let mut n_flags1 = 0;

        while !args.is_empty() {
            let first = args.pop_front().unwrap();
            match Opts::parse(&first, &mut args)? {
                Opts::Model(model) => n_model = model,
                Opts::Help => {
                    return Err(UsageError::HelpRequested);
                }
                Opts::AllResourcesBound => n_flags1 |= D3DCOMPILE_ALL_RESOURCES_BOUND,
                Opts::Define(name, value) => n_defines.push((name, value)),
                Opts::EntryPointName(entry_point) => n_entry_point = entry_point,
                Opts::UnboundedDescriptorTables => {
                    n_flags1 |= D3DCOMPILE_ENABLE_UNBOUNDED_DESCRIPTOR_TABLES
                }
                Opts::OutputFile(output_file) => n_output_file = output_file,
                Opts::BackwardsCompatibility => {
                    n_flags1 |= D3DCOMPILE_ENABLE_BACKWARDS_COMPATIBILITY
                }
                Opts::EnableStrictness => n_flags1 |= D3DCOMPILE_ENABLE_STRICTNESS,
                Opts::AvoidFlowControl => n_flags1 |= D3DCOMPILE_AVOID_FLOW_CONTROL,
                Opts::EnableIEEEStrictness => n_flags1 |= D3DCOMPILE_IEEE_STRICTNESS,
                Opts::PartialPrecision => n_flags1 |= D3DCOMPILE_PARTIAL_PRECISION,
                Opts::NoLogo => (), // ignored
                Opts::DisableOptimizations => n_flags1 |= D3DCOMPILE_SKIP_OPTIMIZATION,
                Opts::DisablePreshaders => n_flags1 |= D3DCOMPILE_NO_PRESHADER,
                Opts::OptimizationLevel0 => n_flags1 |= D3DCOMPILE_OPTIMIZATION_LEVEL0,
                Opts::OptimizationLevel1 => n_flags1 |= D3DCOMPILE_OPTIMIZATION_LEVEL1,
                Opts::OptimizationLevel2 => n_flags1 |= D3DCOMPILE_OPTIMIZATION_LEVEL2,
                Opts::OptimizationLevel3 => n_flags1 |= D3DCOMPILE_OPTIMIZATION_LEVEL3,
                Opts::ResourceMayAlias => n_flags1 |= D3DCOMPILE_RESOURCES_MAY_ALIAS,
                Opts::SkipValidation => n_flags1 |= D3DCOMPILE_SKIP_VALIDATION,
                Opts::OutputIncludeProcessDetails => println!(
                    "option {first} (Output include process details) acknowledged but ignored"
                ),
                Opts::VariableName(variable_name) => n_variable_name = variable_name,
                Opts::WarningsAsErrors => n_flags1 |= D3DCOMPILE_WARNINGS_ARE_ERRORS,
                Opts::DebugInformation => n_flags1 |= D3DCOMPILE_DEBUG,
                Opts::PackMatrixColumnMajor => n_flags1 |= D3DCOMPILE_PACK_MATRIX_COLUMN_MAJOR,
                Opts::PackMatrixRowMajor => n_flags1 |= D3DCOMPILE_PACK_MATRIX_ROW_MAJOR,
                Opts::InputFile(input_file) => {
                    if !n_input_file.is_empty() {
                        return Err(UsageError::TooManyArguments);
                    }
                    n_input_file = input_file;
                }
            }
        }

        // Default initalization and others
        n_defines.shrink_to_fit();
        let mut n_d3d_defines = Vec::with_capacity(n_defines.len() + 1);
        n_defines.iter().for_each(|(name, value)| {
            n_d3d_defines.push(D3D_SHADER_MACRO {
                Name: PCSTR(name.as_bytes_with_nul().as_ptr()),
                Definition: PCSTR(value.as_bytes_with_nul().as_ptr()),
            })
        });
        n_d3d_defines.push(D3D_SHADER_MACRO::default()); // null terminator

        if n_variable_name.is_empty() {
            let entry_point = n_entry_point.to_str().unwrap();
            let model_name = n_model.as_str();
            // if the model doesn't match any from our table, use g_ as the prefix
            let prefix = PROFILE_PREFIX_TABLE
                .iter()
                .find(|i| i.name == model_name)
                .map(|i| i.prefix)
                .unwrap_or("g");
            n_variable_name = format!("{prefix}_{entry_point}");
        }

        eprintln!("option -T (Shader Model/Profile) with arg '{n_model}'",);
        eprintln!("option -E (Entry Point) with arg '{:?}'", n_entry_point);
        eprintln!("option -Fh (Output File) with arg {n_output_file}");
        eprintln!("option -Vn (Variable Name) with arg '{n_variable_name}'");
        eprintln!("option -D (Macro Definition) with args {:?}", n_defines);
        eprintln!("Input file: {n_input_file}");

        Ok(ParseOpt {
            model: n_model,
            entry_point: n_entry_point,
            variable_name: n_variable_name,
            output_file: n_output_file,
            d3d_defines: n_d3d_defines,
            input_file: n_input_file,
            flags1: n_flags1,
        })
    }
    fn compile(self) -> (Result<(), windows::core::Error>, CompileOutput) {
        const D3DCOMPILE_STANDARD_FILE_INCLUDE: &ID3DInclude = unsafe {
            std::mem::transmute::<_, &ID3DInclude>(&(D3D_COMPILE_STANDARD_FILE_INCLUDE as usize))
        };
        let input_data = {
            let mut file = File::open(&self.input_file).expect("Failed to open input file");
            let len = file
                .metadata()
                .expect("Failed to get input file metadata")
                .len();
            let mut data = Vec::with_capacity(len as usize);
            // let mut data = Vec::new();
            file.read_to_end(&mut data)
                .expect("Failed to read input file");
            data
        };
        let file_name = CString::new(self.input_file).unwrap();
        let model = CString::new(self.model).unwrap();

        let mut output: CompileOutput = Default::default();

        // eprintln!("Calling D3DCompile2(");
        // eprintln!("\t{:p},", input_data.as_ptr());
        // eprintln!("\t{},", input_data.len());
        // eprintln!("\t{},", file_name.to_str().unwrap());
        // eprintln!("\t{:p},", self.d3d_defines.as_ptr());
        // eprintln!("\tD3D_COMPILE_STANDARD_FILE_INCLUDE,");
        // eprintln!("\t{},", self.entry_point.to_str().unwrap());
        // eprintln!("\t{},", model.to_str().unwrap());
        // eprintln!("\t0,");
        // eprintln!("\t0,");
        // eprintln!("\t0,");
        // eprintln!("\tNULL,");
        // eprintln!("\t0,");
        // eprintln!("\t{:p},", data.as_mut_ptr());
        // eprintln!("\t{:p})", errors.as_mut_ptr());

        let hr = unsafe {
            D3DCompile2(
                input_data.as_ptr() as *const c_void,
                input_data.len(),
                PCSTR(file_name.as_bytes_with_nul().as_ptr() as *const u8),
                Some(self.d3d_defines.as_ptr()),
                D3DCOMPILE_STANDARD_FILE_INCLUDE,
                PCSTR(self.entry_point.as_bytes_with_nul().as_ptr()),
                PCSTR(model.as_bytes_with_nul().as_ptr()),
                self.flags1,
                0,
                0,
                None,
                0,
                &mut output.data,
                Some(&mut output.errors),
            )
        };
        (hr, output)
    }
}

fn write_output(
    output: ID3DBlob,
    output_file: String,
    variable_name: String,
) -> Result<(), std::io::Error> {
    // Required to ensure crlf output as rust does not do the same conversions
    // that msvcrt does
    const NL: &str = "\r\n";

    // valid until output is released
    let data: &[i8] = unsafe {
        slice::from_raw_parts(
            output.GetBufferPointer() as *const i8,
            output.GetBufferSize(),
        )
    };

    let mut file = File::create(&output_file).expect("Failed to create output file");

    write!(file, "const BYTE {variable_name}[] ={NL}{{{NL}")?;
    for (i, byte) in data.iter().enumerate() {
        write!(file, "{:4}", byte)?;
        if i != data.len() - 1 {
            file.write(b",")?;
        }
        if i % 6 == 5 {
            file.write(NL.as_bytes())?;
        }
    }
    write!(file, "{NL}}};")?;

    eprintln!(
        "Wrote {} bytes of shader output to {}",
        data.len(),
        output_file
    );
    Ok(())
}

fn main() -> ExitCode {
    // ====================================================================================
    // Shader Compilation

    let args = match ParseOpt::new() {
        Ok(args) => args,
        Err(err) => return err.into(),
    };
    let output_file = args.output_file.clone();
    let variable_name = args.variable_name.clone();
    let output = match args.compile() {
        (Ok(()), output) => output,
        (Err(err), output) => {
            eprintln!("Got an error while compiling:");
            eprintln!("{}", err);
            if let Some(errors) = output.errors {
                let error = unsafe { CStr::from_ptr(errors.GetBufferPointer() as *const i8) };
                eprintln!("{}", error.to_string_lossy());
            } else {
                eprintln!("No error message from the function");
            }
            return ExitCode::FAILURE;
        }
    };

    let output = output.data.unwrap();

    match write_output(output, output_file, variable_name) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Failed to write output file:");
            eprintln!("{}", err);
            ExitCode::FAILURE
        }
    }
}
