
function conStruct = createConPart(jsonStruct, bodiesStruct, partSide)
    % ##################################################################################
    % createConPart
    %
    % Creates a structure containing information of connector i and j part, 
    % can be a joint or a bushing defined by the orientation method. 
    %
    % Five orientation methods are currently supported:
    %   - euler         (Euler angles)
    %   - three-points  (Three points)
    %   - two-points    (Two points)
    %   - line-ji       (Line connecting points j and i)
    %   - align-by-reference
    %   - lowest-energy 
    %
    % Inputs:
    %   jsonStruct - Structure containing connector's position and associated body information.
    %   bodiesStruct - Structure detailing all bodies in the system, with specific points and mass properties.
    %   partSide - Specifies the side of the connector part for naming and orientation purposes.
    % Output:
    % -> pointName (string) 
    % -> point (array)
    % -> bodyCM (array)
    % -> orientMethod 
    % -> eulerAngles
    % -> eBasis
    % ->  
    %   conStruct - Structured representation of the connector part, including position, orientation, and geometric properties.
    % ##################################################################################
    
    % Extract position properties from connector struct
    jsonPointName = jsonStruct.position; 
    bodyID = jsonStruct.body;
    orientMethod = jsonStruct.orientation.method;  

    % Adjust for ground body (bodyID == 0) or standard body.
    if bodyID == 0 
        isGround = true;
        conStruct.pointName = generatePointName(jsonPointName, bodyID, partSide, isGround);        
        conStruct.bodyCM = bodiesStruct(length(bodiesStruct)).points.(conStruct.pointName);
        conStruct.point = bodiesStruct(length(bodiesStruct)).points.(conStruct.pointName);
    else
        isGround= false;
        conStruct.pointName = generatePointName(jsonPointName, bodyID, partSide, isGround);        
        conStruct.bodyCM = bodiesStruct(bodyID).cm;
        conStruct.point = bodiesStruct(bodyID).points.(conStruct.pointName);
    end

    conStruct.bodyID = bodyID;

    switch orientMethod

        case 'euler'
            % Extract Euler angles from JSON
            conStruct.orientMethod = 'euler';
            conStruct.euler_angles = jsonStruct.orientation.euler_angles;
            conStruct.eBasis = eulerToRotation(conStruct.euler_angles);
            conStruct.update = isfield(jsonStruct.orientation, 'update');

        case 'three-points' 

            conStruct.orientMethod = 'three-points';
            conStruct.update = isfield(jsonStruct.orientation, 'update');
            
            % Extract point names from JSON
            JsonNameP1 = jsonStruct.orientation.p1;
            JsonNameP2 = jsonStruct.orientation.p2;
            JsonNameP3 = jsonStruct.orientation.p3;

            % Check point location for P1, P2, and P3
            [conStruct.P1, conStruct.NameP1, conStruct.IsGroundP1] = helperCheckPtLocation(bodyID, JsonNameP1, partSide, bodiesStruct);
            [conStruct.P2, conStruct.NameP2, conStruct.IsGroundP2] = helperCheckPtLocation(bodyID, JsonNameP2, partSide, bodiesStruct);
            [conStruct.P3, conStruct.NameP3, conStruct.IsGroundP3] = helperCheckPtLocation(bodyID, JsonNameP3, partSide, bodiesStruct);

            % Extract definition axis
            conStruct.axisP12 = jsonStruct.orientation.axis12;
            conStruct.axisP13 = jsonStruct.orientation.axis13;

            % Create an orthonormal basis using three points
            conStruct.eBasis = threePtsToEbasis(conStruct.P1, conStruct.P2, conStruct.P3, conStruct.axisP12, conStruct.axisP13);

        case 'two-points' 

            conStruct.orientMethod = 'two-points';
            conStruct.update = isfield(jsonStruct.orientation, 'update');
            
            % Extract point names from JSON
            JsonNameP1 = jsonStruct.orientation.p1;
            JsonNameP2 = jsonStruct.orientation.p2;

            % Extract variables defined in Json
            allowedVariables = bodiesStruct(end).points.names; % Always use the last body (ground)
            allowedFunctions = {'cross', 'dot','norm', 'sin', 'cos', 'tan'};

            % Process points P1 and P2
            [conStruct.P1, conStruct.NameP1, conStruct.IsGroundP1] = processPoint(JsonNameP1, allowedVariables, allowedFunctions, bodiesStruct, bodyID, partSide);
            [conStruct.P2, conStruct.NameP2, conStruct.IsGroundP2] = processPoint(JsonNameP2, allowedVariables, allowedFunctions, bodiesStruct, bodyID, partSide);
            
            % Assign axis and helper vectors
            conStruct.axis = jsonStruct.orientation.axis;
            conStruct.helperVec = jsonStruct.orientation.helpvec;
            conStruct.axisOrtho = jsonStruct.orientation.axisortho;
        
            % Create an orthonormal basis using two points
            conStruct.eBasis = twoPtsToEbasis(conStruct.P1, conStruct.P2, conStruct.axis, conStruct.helperVec, conStruct.axisOrtho);

        case 'line-ji'
            conStruct.orientMethod = 'line-ji';

        case 'align-by-reference'
            conStruct.orientMethod = 'align-by-reference';
            conStruct.refNumber = jsonStruct.orientation.number;  
            conStruct.refPart = jsonStruct.orientation.part;

        case 'lowest-energy'
            conStruct.orientMethod = 'lowest-energy';  
            conStruct.kMatrix = jsonStruct.orientation.stiffness;  
            conStruct.euler = jsonStruct.orientation.euler_angles;  
            conStruct.forceConstraint = jsonStruct.orientation.fconstr; % Direction string    
            conStruct.defAxisString = jsonStruct.orientation.defaxis;     % Direction string  
            conStruct.helperVec = jsonStruct.orientation.helpvec;       % Helper vector  
            conStruct.axisOrtho = jsonStruct.orientation.axisortho;
            conStruct.eBasis = deformationToEBasis (conStruct.kMatrix, conStruct.euler, conStruct.forceConstraint, conStruct.defAxisString, conStruct.helperVec, conStruct.axisOrtho);

        otherwise
            error('Invalid orientation method. Supported methods include: euler, three-points, two-points, line-ji, align-by-reference, and lowest-energy.');

    end

    % Calculate distance vector
    conStruct.s = createConDistanceVector(bodiesStruct, conStruct);

end


% #################
% Helper Funnction 
% #################
% -------------------
function [point, pointName, isGround] = helperCheckPtLocation(bodyID, JsonName, partSide, bodiesStruct)
    
    % Generate Point Name based on Body
    pointName = generatePointName(JsonName, bodyID, partSide, false);

    % Check if the point is grounded and adjust if needed
    if bodyID == 0 || ~isfield(bodiesStruct(bodyID).points, pointName)
        isGround = true;
        pointName = generatePointName(JsonName, bodyID, partSide, isGround);
        point = bodiesStruct(end).points.(pointName);
    else
        % Retrieve points
        isGround = false;
        % pointName = pointName;
        point = bodiesStruct(bodyID).points.(pointName);
    end
end
% -------------------

function result = evaluateExpression(expr, allowedVariables, allowedFunctions, bodiesStruct, bodyID, partSide)
    % Initialize variables structure
    variables = struct();

    % Extract all tokens (words) from the expression
    tokens = regexp(expr, '\<[A-Za-z]\w*\>', 'match');
    tokens = unique(tokens); % Remove duplicates
    
    % Prepare the expression for evaluation
    exprToEval = expr;

    % Iterate over each token    
    for i = 1:numel(tokens)
        token = tokens{i};
    
        if ismember(token, allowedVariables)
            % generate point names
            pointName = generatePointName(token, bodyID, partSide, false);

            % Check if the point exists in bodiesStruct
            if isfield(bodiesStruct(bodyID).points, pointName)
                % Assign the point to the variables struct
                variables.(token) = bodiesStruct(bodyID).points.(pointName);
            else
                error('Point "%s" not found in bodiesStruct.', pointName);
            end

            % Replace variable names with their values from the struct
            exprToEval = regexprep(exprToEval, ['\<' token '\>'], ['variables.' token]);

        elseif ismember(token, allowedFunctions)
            % Allowed function; no action needed

        else
            error('Invalid token in expression: %s', token);
        end

    end
    
    % Evaluate the expression safely
    try
        result = eval(exprToEval);
    catch ME
        error('Error evaluating expression: %s', ME.message);
    end
end
% -------------------

function [point, pointName, isGround] = processPoint(JsonPointName, allowedVariables, allowedFunctions, bodiesStruct, bodyID, partSide)
    % Trim any whitespace from the point name
    JsonPointName = strtrim(JsonPointName);

    % Check if the name is an expression defined by (<expression>)
    if startsWith(JsonPointName, '(') && endsWith(JsonPointName, ')')
        % Extract the expression inside the parentheses
        expr = JsonPointName(2:end-1);

        % Evaluate the expression
        point = evaluateExpression(expr, allowedVariables, allowedFunctions, bodiesStruct, bodyID, partSide);
        pointName = expr;
        isGround = false;
    else
        % Use helperCheckPtLocation for direct point names
        [point, pointName, isGround] = helperCheckPtLocation(bodyID, JsonPointName, partSide, bodiesStruct);
    end
end