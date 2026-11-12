function [kVals, currentLoad] = extractStiffness(charStruct, currentDeflection)
    % ##################################################################################
    % Calculates stiffness values based on the type defined within charStruct (spring or bushing).
    %
    % Inputs:
    %   charStruct        - Structure with stiffness characteristics: either single field `k` for spring
    %                       or fields `k1` to `k6` for bushing.
    %   currentDeflection - Array of deflection values corresponding to stiffness values for interpolation.
    %
    % Outputs:
    %   kVals             - Array of calculated stiffness values (1x6 for bushing or 1x1 for spring).
    % ##################################################################################

    % Determine if structure is 'spring' or 'bushing' type
    if isfield(charStruct, 'k') && ~isfield(charStruct, 'k1')
        conType = 'spring';
    elseif isfield(charStruct, 'k1') && isfield(charStruct, 'k6')
        conType = 'bushing';
    else
        error('Invalid type: does not match expected "spring" or "bushing" types.');
    end

    switch conType
        case 'spring'
            % Calculate stiffness for nonlinear spring
            if strcmp(charStruct.k.charType, 'nonlinear')
                kVals = abs(interp1(charStruct.k.arrayGrad(:, 1), charStruct.k.arrayGrad(:, 2), currentDeflection, 'makima', 'extrap'));
                currentLoad = interp1(charStruct.k.array(:, 1), charStruct.k.array(:, 2), currentDeflection, 'linear', 'extrap');
            end

        case 'bushing'
            kVals = zeros(1, 6);  % Initialize stiffness array for bushing

            % Define field names for bushing characteristics (k1 to k6)
            fieldNames = {'k1', 'k2', 'k3', 'k4', 'k5', 'k6'};

            % Loop through each bushing field, calculating stiffness for nonlinear characteristics
            for idx = 1:6
                fieldName = fieldNames{idx};
                if strcmp(charStruct.(fieldName).charType, 'nonlinear')
                    kVals(idx) = abs(interp1(charStruct.(fieldName).arrayGrad(:, 1), charStruct.(fieldName).arrayGrad(:, 2), currentDeflection(idx), 'makima', 'extrap'));
                end
            end
    end
end
%                 % Average Stiffness (really needed?)  
%                 % kAvgVals(idx) = abs(interp1(charStruct.k.SigArray(:,1), charStruct.k.SigArray(:,2), currentDeflection(idx), 'makima', 'extrap')./currentDeflection(idx));
