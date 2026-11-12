function mks_solve_main(modelJson, outJson, designJson)
try
    if nargin >= 3 && ~isempty(designJson)
        history = run_mks_solver(modelJson, readDesign(designJson));
    else
        history = run_mks_solver(modelJson);
    end
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
