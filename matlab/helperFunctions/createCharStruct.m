function charStruct = createCharStruct(stiffnessValues, conType)
    % ##################################################################################
    % Function: createCharStruct
    % Description: Creates a characteristic structure for bushings or springs based on 
    %              stiffness values provided as a JSON file, scalar, or array.
    %
    % Inputs:
    %   stiffnessValues - Either:
    %     - A file path to a JSON file containing bushing/spring properties.
    %     - A scalar stiffness value (applied to all components).
    %     - An array of six linear stiffness values [k1, k2, k3, k4, k5, k6].
    %   conType - Type of connector ('bushing' or 'spring').
    %
    % Outputs:
    %   charStruct - Structure containing characteristics with fields:
    %     - k1 to k6 (for bushing) or k (for spring), each with 'kValue' and 'charType'.
    %     - isLinear: Boolean indicating if all characteristics are linear.
    % ##################################################################################

    % Field names for bushing properties
    kNames = {'k1', 'k2', 'k3', 'k4', 'k5', 'k6'}; 

    % Determine the type of input provided (file or numeric values)
    if (ischar(stiffnessValues) || isstring(stiffnessValues)) && isfile(stiffnessValues)
        propType = 'propertyFile';
    elseif isnumeric(stiffnessValues)
        propType = 'manualEntry';
    else
        error('Invalid input. Provide a valid file path or an array.');
    end

    % Main switch for the connector type
    switch conType
        case 'bushing'
            % Handle the bushing type characteristics
            charStruct = handleBushingType(stiffnessValues, propType, kNames);
        case 'spring'
            % Handle the spring type characteristics
            charStruct = handleSpringType(stiffnessValues, propType);
        otherwise
            % Error for unknown connector types
            error('Unknown connector type defined: %s.', conType);
    end
end

% ##################################################################################
% Helper functions
% ##################################################################################
function charStruct = handleBushingType(stiffnessValues, propType, kNames)
    charStruct = struct();
    
    % Case when stiffness values are provided as a JSON property file
    if strcmp(propType, 'propertyFile')
        % Case when PropertyFile
    else
        for idx = 1:numel(kNames)
            charStruct.k.(kNames{idx}) = struct('kValue', stiffnessValues(idx), 'charType', 'linear');
        end
    end
    
    % Check if all characteristics are linear 
    charStruct.isLinear = all(arrayfun(@(k) strcmp(charStruct.k.(k{1}).charType, 'linear'), kNames));
end

function charStruct = handleSpringType(stiffnessValues, propType)

    charStruct = struct();
    
    % Case when stiffness values are provided as a JSON property file
    if strcmp(propType, 'propertyFile')
        
        % Assign default zero value 
        charStruct.k = struct('kValue', 0, 'charType', 'linear');

        % Load JSON property file -> defined by path string
        jsonStructProp = loadJSON(stiffnessValues); 

        if isfield(jsonStructProp.characteristic, 'k')
            % Extract and assign characteristic type and value
            [charType, kValue] = getCharTypeAndValue(jsonStructProp.characteristic.k, 'k', jsonStructProp.name);
            
            if strcmp(charType,'linear') 
                charStruct.k = struct('kValue', kValue, 'charType', charType);

            else 
                charStruct.k = kValue;
                charStruct.k.charType =charType;
            end

        else
            warning('No k Value for Spring in %s provided; assumed to be zero.', jsonStructProp.name);
        end

    else
        % Case when manual scalar stiffness value is provided
        charStruct.k = struct('kValue', stiffnessValues, 'charType', 'linear');
    end
    
    % Set isLinear based on the characteristic type of 'k'
    charStruct.isLinear = strcmp(charStruct.k.charType, 'linear');
end

function jsonStruct = loadJSON(filePath)
    jsonStruct = jsondecode(fileread(filePath));
end

function [charType, kValue] = getCharTypeAndValue(field, fieldName, namePropFile)
    
    % Default to linear if unspecified
    charType = 'linear';  
    kValue = 0;  
    
    % Check if 'linear' and 'nonlinear' fields exist
    hasLinear = isfield(field, 'linear') && isfield(field.linear, 'active') && field.linear.active;
    hasNonlinear = isfield(field, 'nonlinear') && isfield(field.nonlinear, 'active') && field.nonlinear.active;
    
    % Determine characteristic type
    if hasLinear && ~hasNonlinear
        charType = 'linear';
        kValue = field.linear.data;
        
    elseif hasNonlinear && ~hasLinear
        charType = 'nonlinear';
        xData = field.nonlinear.data.x;
        yData = field.nonlinear.data.y;
        kValue = struct('array', [xData, yData], 'arrayGrad', [xData, gradient(yData, xData)]);
        
    elseif hasLinear && hasNonlinear
        error('Error in Bushing Property File %s: Both linear and nonlinear properties are active for %s.', namePropFile, fieldName);
        
    elseif ~hasLinear && ~hasNonlinear
        warning('Data for %s in %s is set to inactive; defaulting to zero stiffness.', fieldName, namePropFile);
    end
end