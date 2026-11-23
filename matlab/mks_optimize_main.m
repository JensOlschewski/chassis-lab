function mks_optimize_main(problemJson, outJson, varargin)
% mks_solver --optimize <problem.json> <out.json> --design FILE --dry-run
%
% The problem JSON is written by clab from the problem TOML. --dry-run grades
% the start design once, which is also the reference design of the goals. The
% optimization itself is not available yet.
design = [];
dryRun = false;
k = 1;
while k <= numel(varargin)
    switch varargin{k}
        case '--design'
            design = readDesign(varargin{k+1});
            k = k + 2;
        case '--dry-run'
            dryRun = true;
            k = k + 1;
        otherwise
            error('mks:args', 'Unknown option %s', varargin{k});
    end
end
if isempty(design)
    error('mks:args', '--optimize needs --design with the start design');
end
if ~dryRun
    error('mks:args', '--optimize supports only --dry-run so far');
end

problem = jsondecode(fileread(problemJson));
start = startDesign(problem, design);

% The reference design is the start design: models that use design variables
% cannot be solved without one, and goals then mean "better than my baseline".
reference = evaluateProblem(problem, design);
result = gradeProblem(problem, reference, reference);
result.design = start;

fid = fopen(outJson, 'w');
fwrite(fid, jsonencode(result, 'PrettyPrint', true));
fclose(fid);
end

function start = startDesign(problem, design)
% The start value of every design variable, checked against its bounds and
% against the models that use it.
texts = cellfun(@fileread, struct2cell(problem.models), 'UniformOutput', false);
start = struct();
for k = 1:numel(problem.design)
    d = problem.design(k);
    if ~isKey(design, d.name)
        error('mks:design', 'Design variable %s has no start value in --design', d.name);
    end
    value = design(d.name);
    value = value{1};
    if ~isnumeric(value) || ~isscalar(value)
        error('mks:design', 'Design variable %s must be one number', d.name);
    end
    if value < d.lower || value > d.upper
        error('mks:design', 'Start value %g of %s is outside [%g, %g]', value, d.name, d.lower, d.upper);
    end
    pattern = "\<" + regexptranslate('escape', d.name) + "\>";
    if ~any(cellfun(@(text) ~isempty(regexp(text, pattern, 'once')), texts))
        error('mks:design', 'Design variable %s is not used by any model', d.name);
    end
    start.(d.name) = value;
end
end
