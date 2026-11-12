function mks_solve_main(modelJson, outJson, varargin)
% mks_solver <model.json> <out.json> [--design FILE] [--include a,b,c]
% --include lists the optional outputs (bodies, connectors, suspension).
% Outputs not listed are off, whatever the model JSON says.
try
    design = [];
    include = strings(0);
    for k = 1:2:numel(varargin)
        switch varargin{k}
            case '--design'
                design = readDesign(varargin{k+1});
            case '--include'
                include = split(string(varargin{k+1}), ',');
            otherwise
                error('mks:args', 'Unknown option %s', varargin{k});
        end
    end
    history = run_mks_solver(modelJson, design, include);
    fid = fopen(outJson, 'w');
    fwrite(fid, jsonencode(history));
    fclose(fid);
catch err
    fprintf(2, '%s\n%s\n', err.identifier, err.message);
    exit(1);
end
end

function design = readDesign(file)
s = jsondecode(fileread(file));
names = string(fieldnames(s));
values = arrayfun(@(n) double(s.(n)), names);
design = dictionary(names, values);
end
